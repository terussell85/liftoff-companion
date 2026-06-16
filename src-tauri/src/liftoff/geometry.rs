//! Scene-graph collision geometry extraction and telemetry-event confirmation.
//!
//! The extractor uses memory-mapped UnityFS files plus lazy block extraction so
//! asset refresh can scan large installs without forcing entire bundles into
//! heap memory. Only explicit collider components from resolved scene/prefab
//! graphs are used to confirm collision events.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use base64::{engine::general_purpose, Engine as _};
use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use unity_asset::UnityValue;
use unity_asset_binary::asset::{SerializedFile, SerializedFileParser};
use unity_asset_binary::bundle::BundleLoadOptions;
use unity_asset_binary::file::load_bundle_file_with_options;

use crate::error::AppResult;
use crate::liftoff::assets::{ReplayCourseData, ReplayCourseProp};
use crate::processing::collisions::CollisionEvent;
use crate::storage::repositories::{self, CollisionGeometryCacheRow};
use crate::telemetry::sample::TelemetrySample;

const GEOMETRY_VERSION: &str = "collision-geometry-v9";
const SCOPE_ENVIRONMENT: &str = "environment";
const SCOPE_RACE: &str = "race";
const SCOPE_ITEM: &str = "item";
const PROVENANCE_SCENE_GRAPH: &str = "scene_graph";
const PROVENANCE_PREFAB_GRAPH: &str = "prefab_graph";
const PROVENANCE_TRACK_XML_PROCEDURAL: &str = "track_xml_procedural";
const MAX_NODE_BYTES: u64 = 1536 * 1024 * 1024;
const MAX_LAZY_BLOCK_CACHE_BYTES: usize = 256 * 1024 * 1024;
const MAX_CATALOG_WINDOW_BYTES: usize = 1_500_000;
const MAX_CANDIDATE_BUNDLES: usize = 24;
const MAX_DECODED_ENVIRONMENT_CANDIDATE_BUNDLES: usize = 128;
const MAX_DECODED_ITEM_CANDIDATE_BUNDLES: usize = 24;
const MAX_DEPENDENCY_BUNDLES_PER_GROUP: usize = 16;
const MAX_SHAPES_PER_SCOPE: usize = 500_000;
const MIN_ENVIRONMENT_SCENE_SHAPES_TO_COMPLETE: usize = 128;
const DRONE_RADIUS_METERS: f32 = 0.12;
const CONTACT_MARGIN_METERS: f32 = 0.25;
const PROCEDURAL_RIBBON_RADIUS_METERS: f32 = 0.10;
const NON_PHYSICAL_COLLIDER_LABEL_PARTS: &[&str] = &[
    "lightprobe",
    "reflectionprobe",
    "occlusion",
    "postprocess",
    "navmesh",
];

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GeometryScope {
    pub kind: String,
    pub id: String,
}

impl GeometryScope {
    pub fn environment(id: impl Into<String>) -> Self {
        Self {
            kind: SCOPE_ENVIRONMENT.into(),
            id: id.into(),
        }
    }

    pub fn item(id: impl Into<String>) -> Self {
        Self {
            kind: SCOPE_ITEM.into(),
            id: id.into(),
        }
    }

    pub fn race(id: impl Into<String>) -> Self {
        Self {
            kind: SCOPE_RACE.into(),
            id: id.into(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionGeometryDocument {
    pub version: String,
    pub scope_kind: String,
    pub scope_id: String,
    pub coordinate_space: String,
    pub shapes: Vec<CollisionShape>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionShape {
    pub id: String,
    pub source_kind: String,
    pub source_id: String,
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub provenance: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub source_asset: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub object_path: Option<String>,
    pub shape: String,
    pub center: [f32; 3],
    pub half_extents: [f32; 3],
    #[serde(default = "identity_rotation_array")]
    pub rotation: [f32; 4],
    pub confidence: f32,
}

fn identity_rotation_array() -> [f32; 4] {
    [0.0, 0.0, 0.0, 1.0]
}

#[derive(Debug, Clone)]
pub struct CourseCollisionGeometry {
    pub shapes: Vec<CollisionShape>,
    pub warnings: Vec<String>,
    pub unavailable: bool,
}

#[derive(Debug, Clone)]
pub struct GeometryExtractionProgress {
    pub phase: String,
    pub scopes_done: u64,
    pub scopes_total: u64,
    pub levels_done: u64,
    pub levels_total: u64,
    pub bundles_done: u64,
    pub bundles_total: u64,
    pub current_scope_kind: Option<String>,
    pub current_scope_id: Option<String>,
    pub current_level: Option<String>,
    pub current_bundle: Option<String>,
    pub shapes_found: u64,
}

#[derive(Debug, Clone)]
struct AddressablesIndex {
    catalog: Vec<u8>,
    bundle_positions: Vec<(usize, String)>,
    bundle_paths: Vec<PathBuf>,
    bundles_by_name: HashMap<String, PathBuf>,
    decoded_catalogs: Vec<DecodedAddressablesCatalog>,
    binary_catalogs: Vec<BinaryAddressablesCatalog>,
}

#[derive(Debug, Clone)]
struct DecodedAddressablesCatalog {
    keys: Vec<Option<String>>,
    normalized_keys: Vec<Option<String>>,
    key_indices_by_normalized_value: HashMap<String, Vec<usize>>,
    buckets: Vec<Vec<usize>>,
    entries: Vec<[i32; 7]>,
    internal_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct BinaryAddressablesCatalog {
    bytes: Vec<u8>,
    keys: Vec<Option<String>>,
    normalized_keys: Vec<Option<String>>,
    key_indices_by_normalized_value: HashMap<String, Vec<usize>>,
    location_set_offsets: Vec<u32>,
}

#[derive(Debug, Deserialize)]
struct AddressablesCatalogJson {
    #[serde(rename = "m_InternalIds")]
    internal_ids: Vec<String>,
    #[serde(rename = "m_KeyDataString")]
    key_data: String,
    #[serde(rename = "m_BucketDataString")]
    bucket_data: String,
    #[serde(rename = "m_EntryDataString")]
    entry_data: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct CatalogKeyHit {
    rank: usize,
    key_len: usize,
    index: usize,
}

#[derive(Debug)]
struct ScopeExtractionState {
    kind: String,
    id: String,
    aliases: Vec<String>,
    candidates: Vec<PathBuf>,
    doc: CollisionGeometryDocument,
    source_bundle: Option<String>,
    source_hash: Option<String>,
    started: bool,
    direct_candidates_done: usize,
    completed_direct_candidate_paths: HashSet<PathBuf>,
    done: bool,
    completed: bool,
}

#[derive(Debug, Clone)]
struct RawShapeCandidate {
    file_key: usize,
    label: String,
    hierarchy_path: String,
    shape_name: String,
    center: Vec3,
    half_extents: Vec3,
    rotation: Quat,
    confidence: f32,
}

#[derive(Debug, Clone)]
struct BundleGroupShapeCandidates {
    candidates: Vec<RawShapeCandidate>,
    file_markers: Vec<HashSet<String>>,
    file_bundle_keys: Vec<usize>,
    warnings: Vec<String>,
}

#[derive(Debug, Clone)]
struct CachedBundleGeometry {
    parsed: Option<ParsedBundleGeometry>,
    warnings: Vec<String>,
}

#[derive(Debug)]
struct ArchiveBundleResolver {
    bundle_paths: Vec<PathBuf>,
    archive_bundle_cache: HashMap<String, Option<PathBuf>>,
}

pub fn extract_collision_geometry_cache(
    data_root: &Path,
    cache_id: &str,
    scopes: impl IntoIterator<Item = GeometryScope>,
    mut progress: impl FnMut(GeometryExtractionProgress),
) -> Vec<CollisionGeometryCacheRow> {
    let index = AddressablesIndex::from_data_root(data_root);
    let mut seen = BTreeSet::new();
    let scopes = scopes
        .into_iter()
        .filter_map(|scope| {
            let scope_id = scope.id.trim();
            if scope_id.is_empty() || !seen.insert((scope.kind.clone(), scope_id.to_string())) {
                return None;
            }
            Some(GeometryScope {
                kind: scope.kind,
                id: scope_id.to_string(),
            })
        })
        .collect::<Vec<_>>();
    let mut states = scopes
        .into_iter()
        .map(|scope| ScopeExtractionState::new(scope, &index))
        .collect::<Vec<_>>();
    let scopes_total = states.len() as u64;
    let mut tracker = GeometryProgressTracker::new(&states);

    tracker.emit(&mut progress, "geometry_started", &states, None, None, None);

    extract_scope_states(&mut states, &index, &mut tracker, &mut progress);

    tracker.scopes_done = scopes_total;
    tracker.emit(
        &mut progress,
        "geometry_completed",
        &states,
        None,
        None,
        None,
    );

    let rows = states
        .into_iter()
        .map(|state| state.into_row(cache_id))
        .collect::<Vec<_>>();

    rows
}

impl ScopeExtractionState {
    fn new(scope: GeometryScope, index: &AddressablesIndex) -> Self {
        let coordinate_space = if scope.kind == SCOPE_ENVIRONMENT || scope.kind == SCOPE_RACE {
            "world"
        } else {
            "local"
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: scope.kind.clone(),
            scope_id: scope.id.clone(),
            coordinate_space: coordinate_space.into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };
        let candidates = index.candidate_bundles(&scope.id, scope.kind == SCOPE_ENVIRONMENT);
        let aliases = catalog_candidate_aliases(&scope.id, scope.kind == SCOPE_ENVIRONMENT)
            .into_iter()
            .map(str::to_string)
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            doc.warnings.push(format!(
                "No Addressables bundle candidates found for {}:{}.",
                scope.kind, scope.id
            ));
        }

        Self {
            kind: scope.kind,
            id: scope.id,
            aliases,
            candidates,
            doc,
            source_bundle: None,
            source_hash: None,
            started: false,
            direct_candidates_done: 0,
            completed_direct_candidate_paths: HashSet::new(),
            done: false,
            completed: false,
        }
    }

    fn into_row(self, cache_id: &str) -> CollisionGeometryCacheRow {
        let status = if self.doc.shapes.is_empty() {
            "missing"
        } else if self.doc.warnings.is_empty() {
            "ready"
        } else {
            "partial"
        };
        let warning_count = self.doc.warnings.len() as i64;
        let geometry_json = serde_json::to_string(&self.doc).unwrap_or_else(|_| "{}".into());

        CollisionGeometryCacheRow {
            cache_id: cache_id.into(),
            scope_kind: self.kind,
            scope_id: self.id,
            geometry_json,
            status: status.into(),
            source_bundle: self.source_bundle,
            source_hash: self.source_hash,
            warning_count,
            error_message: None,
            extracted_at: Some(Utc::now()),
        }
    }
}

#[derive(Debug)]
struct GeometryProgressTracker {
    scopes_done: u64,
    scopes_total: u64,
    bundles_done: u64,
    bundles_total: u64,
    direct_bundle_paths: HashSet<PathBuf>,
    completed_direct_bundles: HashSet<PathBuf>,
    shared_group_totals: HashMap<Vec<PathBuf>, u64>,
    completed_shared_groups: HashSet<Vec<PathBuf>>,
}

impl GeometryProgressTracker {
    fn new(states: &[ScopeExtractionState]) -> Self {
        let shared_group_totals = potential_shared_item_group_totals(states);
        let direct_bundle_paths = states
            .iter()
            .flat_map(|state| state.candidates.iter().cloned())
            .collect::<HashSet<_>>();
        let direct_total = direct_bundle_paths.len() as u64;
        let shared_total = shared_group_totals.values().sum::<u64>();

        Self {
            scopes_done: 0,
            scopes_total: states.len() as u64,
            bundles_done: 0,
            bundles_total: direct_total + shared_total,
            direct_bundle_paths,
            completed_direct_bundles: HashSet::new(),
            shared_group_totals,
            completed_shared_groups: HashSet::new(),
        }
    }

    fn emit(
        &self,
        progress: &mut impl FnMut(GeometryExtractionProgress),
        phase: &str,
        states: &[ScopeExtractionState],
        current_scope_kind: Option<String>,
        current_scope_id: Option<String>,
        current_bundle: Option<String>,
    ) {
        let levels_done = states
            .iter()
            .filter(|state| state.kind == SCOPE_ENVIRONMENT && state.completed)
            .count() as u64;
        let levels_total = states
            .iter()
            .filter(|state| state.kind == SCOPE_ENVIRONMENT)
            .count() as u64;
        let current_level = match (current_scope_kind.as_deref(), current_scope_id.as_deref()) {
            (Some(SCOPE_ENVIRONMENT), Some(id)) => Some(id.to_string()),
            _ => None,
        };

        progress(GeometryExtractionProgress {
            phase: phase.into(),
            scopes_done: self.scopes_done,
            scopes_total: self.scopes_total,
            levels_done,
            levels_total,
            bundles_done: self.bundles_done,
            bundles_total: self.bundles_total,
            current_scope_kind,
            current_scope_id,
            current_level,
            current_bundle,
            shapes_found: total_shapes_found(states),
        });
    }

    fn complete_bundle_work(&mut self, count: u64) {
        self.bundles_done = self
            .bundles_done
            .saturating_add(count)
            .min(self.bundles_total);
    }

    fn complete_direct_bundle(&mut self, path: &Path) {
        if self.direct_bundle_paths.contains(path)
            && self.completed_direct_bundles.insert(path.to_path_buf())
        {
            self.complete_bundle_work(1);
        }
    }

    fn skip_inactive_direct_bundles(&mut self, states: &[ScopeExtractionState]) {
        let active_paths = states
            .iter()
            .filter(|state| !state.done && !state.completed)
            .flat_map(|state| {
                state.candidates.iter().filter(|path| {
                    !state
                        .completed_direct_candidate_paths
                        .contains(path.as_path())
                })
            })
            .cloned()
            .collect::<HashSet<_>>();
        let inactive_paths = self
            .direct_bundle_paths
            .iter()
            .filter(|path| {
                !self.completed_direct_bundles.contains(path.as_path())
                    && !active_paths.contains(path.as_path())
            })
            .cloned()
            .collect::<Vec<_>>();

        for path in inactive_paths {
            self.complete_direct_bundle(&path);
        }
    }

    fn skip_inactive_shared_groups(&mut self, active_groups: &HashSet<Vec<PathBuf>>) {
        let inactive = self
            .shared_group_totals
            .keys()
            .filter(|paths| !active_groups.contains(*paths))
            .cloned()
            .collect::<Vec<_>>();

        for paths in inactive {
            self.complete_shared_group(&paths);
        }
    }

    fn complete_shared_bundle(&mut self) {
        self.complete_bundle_work(1);
    }

    fn complete_shared_group(&mut self, paths: &[PathBuf]) {
        let key = paths.to_vec();
        if !self.completed_shared_groups.insert(key.clone()) {
            return;
        }
        let total = self
            .shared_group_totals
            .get(&key)
            .copied()
            .unwrap_or(paths.len() as u64);
        self.complete_bundle_work(total);
    }
}

fn extract_scope_states(
    states: &mut [ScopeExtractionState],
    index: &AddressablesIndex,
    tracker: &mut GeometryProgressTracker,
    progress: &mut impl FnMut(GeometryExtractionProgress),
) {
    let mut bundle_cache = HashMap::<PathBuf, CachedBundleGeometry>::new();
    let mut archive_resolver = ArchiveBundleResolver::new(index);
    let scope_indices_by_bundle = direct_scope_indices_by_bundle(states);
    let max_candidates = states
        .iter()
        .map(|state| state.candidates.len())
        .max()
        .unwrap_or(0);

    for candidate_index in 0..max_candidates {
        let mut bundle_paths = Vec::<PathBuf>::new();
        let mut queued_paths = HashSet::<PathBuf>::new();

        for scope_index in 0..states.len() {
            if states[scope_index].done {
                continue;
            }
            if !states[scope_index].started {
                states[scope_index].started = true;
                tracker.emit(
                    progress,
                    "geometry_scope_started",
                    states,
                    Some(states[scope_index].kind.clone()),
                    Some(states[scope_index].id.clone()),
                    None,
                );
            }
            if states[scope_index].doc.shapes.len() >= MAX_SHAPES_PER_SCOPE {
                states[scope_index].doc.warnings.push(format!(
                    "Shape limit reached for {}:{}; remaining bundles skipped.",
                    states[scope_index].kind, states[scope_index].id
                ));
                states[scope_index].done = true;
                complete_scope(states, scope_index, tracker, progress);
                continue;
            }
            let Some(path) = states[scope_index].candidates.get(candidate_index).cloned() else {
                states[scope_index].done = true;
                if states[scope_index].kind == SCOPE_ITEM
                    && states[scope_index].doc.shapes.is_empty()
                {
                    continue;
                }
                complete_scope(states, scope_index, tracker, progress);
                continue;
            };

            if states[scope_index]
                .completed_direct_candidate_paths
                .contains(&path)
            {
                continue;
            }
            if queued_paths.insert(path.clone()) {
                bundle_paths.push(path);
            }
        }

        for path in bundle_paths {
            let scope_indices =
                pending_scope_indices_for_bundle(states, &scope_indices_by_bundle, &path);
            if scope_indices.is_empty() {
                tracker.skip_inactive_direct_bundles(states);
                continue;
            }

            tracker.emit(
                progress,
                "geometry_bundle_started",
                states,
                None,
                None,
                Some(bundle_label(&path)),
            );

            let before = scope_indices
                .iter()
                .map(|idx| (*idx, states[*idx].doc.shapes.len()))
                .collect::<Vec<_>>();
            extract_scopes_from_cached_bundle(
                &path,
                &scope_indices,
                states,
                &mut bundle_cache,
                &mut archive_resolver,
            );

            let mut completed_scopes = Vec::new();
            for (idx, before_len) in before {
                if states[idx].doc.shapes.len() > before_len {
                    let item_complete = states[idx].kind == SCOPE_ITEM;
                    let race_scene_complete = states[idx].kind == SCOPE_RACE;
                    let environment_scene_complete = states[idx].kind == SCOPE_ENVIRONMENT
                        && states[idx].doc.shapes.len() >= MIN_ENVIRONMENT_SCENE_SHAPES_TO_COMPLETE;
                    if item_complete || race_scene_complete || environment_scene_complete {
                        states[idx]
                            .source_bundle
                            .get_or_insert_with(|| path.to_string_lossy().to_string());
                        states[idx]
                            .source_hash
                            .get_or_insert_with(|| bundle_metadata_hash(&path));
                        states[idx].done = true;
                        completed_scopes.push(idx);
                    }
                }
            }

            for idx in &scope_indices {
                mark_direct_candidate_processed(&mut states[*idx], &path);
            }
            tracker.complete_direct_bundle(&path);
            tracker.emit(
                progress,
                "geometry_bundle_completed",
                states,
                None,
                None,
                Some(bundle_label(&path)),
            );

            for idx in completed_scopes {
                complete_scope(states, idx, tracker, progress);
            }
            tracker.skip_inactive_direct_bundles(states);
        }
        tracker.skip_inactive_direct_bundles(states);
    }

    tracker.skip_inactive_direct_bundles(states);
    extract_missing_item_candidate_groups(
        states,
        tracker,
        progress,
        &mut bundle_cache,
        &mut archive_resolver,
    );

    for scope_index in 0..states.len() {
        if states[scope_index].completed {
            continue;
        }
        states[scope_index].done = true;
        complete_scope(states, scope_index, tracker, progress);
    }
}

fn complete_scope(
    states: &mut [ScopeExtractionState],
    scope_index: usize,
    tracker: &mut GeometryProgressTracker,
    progress: &mut impl FnMut(GeometryExtractionProgress),
) {
    if states[scope_index].completed {
        return;
    }
    states[scope_index].direct_candidates_done = states[scope_index].candidates.len();
    states[scope_index].completed = true;
    tracker.skip_inactive_direct_bundles(states);
    tracker.scopes_done = (tracker.scopes_done + 1).min(tracker.scopes_total);
    tracker.emit(
        progress,
        "geometry_scope_completed",
        states,
        Some(states[scope_index].kind.clone()),
        Some(states[scope_index].id.clone()),
        None,
    );
}

fn total_shapes_found(states: &[ScopeExtractionState]) -> u64 {
    states
        .iter()
        .map(|state| state.doc.shapes.len() as u64)
        .sum()
}

fn potential_shared_item_group_totals(
    states: &[ScopeExtractionState],
) -> HashMap<Vec<PathBuf>, u64> {
    let mut groups = HashMap::<Vec<PathBuf>, u64>::new();
    for state in states {
        if state.kind != SCOPE_ITEM || state.candidates.is_empty() {
            continue;
        }
        groups
            .entry(state.candidates.clone())
            .or_insert(state.candidates.len() as u64);
    }
    groups
}

fn direct_scope_indices_by_bundle(states: &[ScopeExtractionState]) -> HashMap<PathBuf, Vec<usize>> {
    let mut out = HashMap::<PathBuf, Vec<usize>>::new();
    for (idx, state) in states.iter().enumerate() {
        let mut seen_paths = HashSet::new();
        for path in &state.candidates {
            if seen_paths.insert(path) {
                out.entry(path.clone()).or_default().push(idx);
            }
        }
    }
    out
}

fn pending_scope_indices_for_bundle(
    states: &[ScopeExtractionState],
    scope_indices_by_bundle: &HashMap<PathBuf, Vec<usize>>,
    path: &Path,
) -> Vec<usize> {
    scope_indices_by_bundle
        .get(path)
        .into_iter()
        .flatten()
        .filter_map(|idx| {
            let state = &states[*idx];
            if state.done
                || state.completed
                || state.doc.shapes.len() >= MAX_SHAPES_PER_SCOPE
                || state.completed_direct_candidate_paths.contains(path)
            {
                return None;
            }
            Some(*idx)
        })
        .collect()
}

fn mark_direct_candidate_processed(state: &mut ScopeExtractionState, path: &Path) {
    if state
        .completed_direct_candidate_paths
        .insert(path.to_path_buf())
    {
        state.direct_candidates_done = state
            .direct_candidates_done
            .saturating_add(1)
            .min(state.candidates.len());
    }
}

fn extract_scopes_from_cached_bundle(
    path: &Path,
    scope_indices: &[usize],
    states: &mut [ScopeExtractionState],
    cache: &mut HashMap<PathBuf, CachedBundleGeometry>,
    archive_resolver: &mut ArchiveBundleResolver,
) {
    let source_paths = vec![path.to_path_buf()];
    ensure_cached_bundle_paths(&source_paths, cache);
    let (paths, dependency_warnings) =
        dependency_enriched_bundle_paths(&source_paths, cache, archive_resolver);
    let candidates = extract_shape_candidates_from_cached_paths(&paths, cache, dependency_warnings);
    for idx in scope_indices {
        for warning in &candidates.warnings {
            states[*idx].doc.warnings.push(warning.clone());
        }
        let state = &mut states[*idx];
        append_scope_shape_candidates(
            &candidates,
            &state.kind,
            &state.id,
            &state.aliases,
            &mut state.doc,
        );
    }
}

fn extract_missing_item_candidate_groups(
    states: &mut [ScopeExtractionState],
    tracker: &mut GeometryProgressTracker,
    progress: &mut impl FnMut(GeometryExtractionProgress),
    cache: &mut HashMap<PathBuf, CachedBundleGeometry>,
    archive_resolver: &mut ArchiveBundleResolver,
) {
    let mut groups = HashMap::<Vec<PathBuf>, Vec<usize>>::new();
    for (idx, state) in states.iter().enumerate() {
        if state.kind != SCOPE_ITEM || !state.doc.shapes.is_empty() || state.candidates.is_empty() {
            continue;
        }
        groups
            .entry(state.candidates.clone())
            .or_default()
            .push(idx);
    }

    let active_groups = groups.keys().cloned().collect::<HashSet<_>>();
    tracker.skip_inactive_shared_groups(&active_groups);

    if groups.is_empty() {
        return;
    }

    for (paths, scope_indices) in groups {
        let current_scope_id = scope_indices.first().map(|idx| states[*idx].id.clone());
        tracker.emit(
            progress,
            "geometry_item_group_started",
            states,
            Some(SCOPE_ITEM.into()),
            current_scope_id.clone(),
            paths.first().map(|path| bundle_label(path)),
        );

        let mut group_progress = |phase: &str, _bundle_index: usize, path: &Path| {
            if phase.ends_with("_completed") {
                tracker.complete_shared_bundle();
            }
            tracker.emit(
                progress,
                phase,
                states,
                Some(SCOPE_ITEM.into()),
                current_scope_id.clone(),
                Some(bundle_label(path)),
            );
        };
        let group_candidates = extract_shape_candidates_from_bundle_group(
            &paths,
            cache,
            archive_resolver,
            &mut group_progress,
        );

        let mut completed_scopes = Vec::new();
        for idx in &scope_indices {
            let before = states[*idx].doc.shapes.len();
            for warning in &group_candidates.warnings {
                states[*idx].doc.warnings.push(warning.clone());
            }
            append_scope_shape_candidates(
                &group_candidates,
                &states[*idx].kind,
                &states[*idx].id,
                &states[*idx].aliases,
                &mut states[*idx].doc,
            );
            if states[*idx].doc.shapes.len() > before {
                if let Some(path) = paths.first() {
                    states[*idx]
                        .source_bundle
                        .get_or_insert_with(|| path.to_string_lossy().to_string());
                    states[*idx]
                        .source_hash
                        .get_or_insert_with(|| bundle_metadata_hash(path));
                }
                states[*idx].done = true;
                completed_scopes.push(*idx);
            }
        }

        for idx in completed_scopes {
            complete_scope(states, idx, tracker, progress);
        }
    }
}

fn extract_shape_candidates_from_bundle_group(
    paths: &[PathBuf],
    cache: &mut HashMap<PathBuf, CachedBundleGeometry>,
    archive_resolver: &mut ArchiveBundleResolver,
    progress: &mut impl FnMut(&str, usize, &Path),
) -> BundleGroupShapeCandidates {
    for (path_index, path) in paths.iter().enumerate() {
        progress("geometry_item_group_bundle_started", path_index, path);
        let _ = cached_bundle_geometry(path, cache);
        progress("geometry_item_group_bundle_completed", path_index, path);
    }

    let (paths, dependency_warnings) =
        dependency_enriched_bundle_paths(paths, cache, archive_resolver);
    extract_shape_candidates_from_cached_paths(&paths, cache, dependency_warnings)
}

fn ensure_cached_bundle_paths(
    paths: &[PathBuf],
    cache: &mut HashMap<PathBuf, CachedBundleGeometry>,
) {
    for path in paths {
        let _ = cached_bundle_geometry(path, cache);
    }
}

fn dependency_enriched_bundle_paths(
    paths: &[PathBuf],
    cache: &mut HashMap<PathBuf, CachedBundleGeometry>,
    archive_resolver: &mut ArchiveBundleResolver,
) -> (Vec<PathBuf>, Vec<String>) {
    let mut out = paths.to_vec();
    let mut seen = out.iter().cloned().collect::<HashSet<_>>();
    let mut warnings = Vec::new();

    ensure_cached_bundle_paths(&out, cache);
    let archive_ids = unresolved_mesh_dependency_archive_ids_for_paths(&out, cache);
    for archive_id in archive_ids.into_iter().take(MAX_DEPENDENCY_BUNDLES_PER_GROUP) {
        match archive_resolver.find_bundle_for_archive(&archive_id) {
            Some(path) if seen.insert(path.clone()) => out.push(path),
            Some(_) => {}
            None => warnings.push(format!(
                "Mesh dependency archive {archive_id} was not found; some mesh colliders may be skipped."
            )),
        }
    }
    ensure_cached_bundle_paths(&out, cache);

    (out, warnings)
}

fn extract_shape_candidates_from_cached_paths(
    paths: &[PathBuf],
    cache: &HashMap<PathBuf, CachedBundleGeometry>,
    mut warnings: Vec<String>,
) -> BundleGroupShapeCandidates {
    let mut bundles = Vec::<&ParsedBundleGeometry>::new();
    for path in paths {
        let Some(cached) = cache.get(path) else {
            continue;
        };
        warnings.extend(cached.warnings.clone());
        if let Some(bundle) = cached.parsed.as_ref() {
            bundles.push(bundle);
        }
    }

    extract_shape_candidates_from_parsed_bundles(bundles, warnings)
}

fn cached_bundle_geometry<'a>(
    path: &Path,
    cache: &'a mut HashMap<PathBuf, CachedBundleGeometry>,
) -> &'a CachedBundleGeometry {
    cache
        .entry(path.to_path_buf())
        .or_insert_with(|| match parse_bundle_geometry(path) {
            Ok((parsed, warnings)) => CachedBundleGeometry {
                parsed: Some(parsed),
                warnings,
            },
            Err(error) => CachedBundleGeometry {
                parsed: None,
                warnings: vec![format!(
                    "{}: {error}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("bundle")
                )],
            },
        })
}

fn unresolved_mesh_dependency_archive_ids_for_paths(
    paths: &[PathBuf],
    cache: &HashMap<PathBuf, CachedBundleGeometry>,
) -> BTreeSet<String> {
    let bundles = paths
        .iter()
        .filter_map(|path| cache.get(path))
        .filter_map(|cached| cached.parsed.as_ref())
        .collect::<Vec<_>>();
    unresolved_mesh_dependency_archive_ids(bundles)
}

fn unresolved_mesh_dependency_archive_ids<'a>(
    bundles: impl IntoIterator<Item = &'a ParsedBundleGeometry>,
) -> BTreeSet<String> {
    let bundles = bundles.into_iter().collect::<Vec<_>>();
    let available_archives = bundles
        .iter()
        .flat_map(|bundle| bundle.files.iter())
        .filter_map(|file| file.archive_id.as_ref())
        .cloned()
        .collect::<HashSet<_>>();
    let mut out = BTreeSet::new();

    for file in bundles.iter().flat_map(|bundle| bundle.files.iter()) {
        for collider in &file.colliders {
            let Some(mesh_ref) = collider.mesh_ref() else {
                continue;
            };
            if mesh_ref.file_id <= 0 {
                continue;
            }
            let Some(external_index) = mesh_ref
                .file_id
                .checked_sub(1)
                .and_then(|idx| usize::try_from(idx).ok())
            else {
                continue;
            };
            let Some(Some(archive_id)) = file.external_archives.get(external_index) else {
                continue;
            };
            if !available_archives.contains(archive_id) {
                out.insert(archive_id.clone());
            }
        }
    }

    out
}

fn extract_shape_candidates_from_parsed_bundles<'a>(
    bundles: impl IntoIterator<Item = &'a ParsedBundleGeometry>,
    warnings: Vec<String>,
) -> BundleGroupShapeCandidates {
    let mut files = Vec::<&ParsedFileGeometry>::new();
    let mut file_bundle_keys = Vec::new();
    for (bundle_key, bundle) in bundles.into_iter().enumerate() {
        for file in &bundle.files {
            files.push(file);
            file_bundle_keys.push(bundle_key);
        }
    }

    let mut archive_to_file = HashMap::<String, usize>::new();
    let mut mesh_lookup = HashMap::<(usize, i64), MeshInfo>::new();
    for (file_key, file) in files.iter().enumerate() {
        if let Some(archive_id) = &file.archive_id {
            archive_to_file
                .entry(archive_id.clone())
                .or_insert(file_key);
        }
        for (path_id, mesh) in &file.meshes {
            mesh_lookup.insert((file_key, *path_id), mesh.clone());
        }
    }

    let file_markers = files
        .iter()
        .map(|file| file.markers.clone())
        .collect::<Vec<_>>();
    let mut candidates = Vec::new();
    for (file_key, file) in files.iter().enumerate() {
        candidates.extend(extract_shape_candidates_from_parsed_file(
            file_key,
            file,
            &mesh_lookup,
            &archive_to_file,
        ));
    }
    BundleGroupShapeCandidates {
        candidates,
        file_markers,
        file_bundle_keys,
        warnings,
    }
}

fn parse_bundle_geometry(path: &Path) -> anyhow::Result<(ParsedBundleGeometry, Vec<String>)> {
    let mut options = BundleLoadOptions::lazy();
    options.max_memory = Some(MAX_NODE_BYTES as usize);
    options.max_unityfs_block_cache_memory = Some(MAX_LAZY_BLOCK_CACHE_BYTES);
    options.max_compressed_block_size = Some(MAX_NODE_BYTES as usize);

    let bundle = load_bundle_file_with_options(path, options)?;
    let mut files = Vec::new();
    let mut warnings = Vec::new();
    for node in &bundle.nodes {
        if is_resource_payload_node(&node.name) {
            continue;
        }
        if node.size > MAX_NODE_BYTES {
            warnings.push(format!(
                "{} node {} skipped: {} bytes exceeds {} byte limit.",
                path.file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or("bundle"),
                node.name,
                node.size,
                MAX_NODE_BYTES
            ));
            continue;
        }
        let node_bytes = match bundle.extract_node_data(node) {
            Ok(bytes) => bytes,
            Err(error) => {
                warnings.push(format!(
                    "{} node {} skipped: {error}",
                    path.file_name()
                        .and_then(|name| name.to_str())
                        .unwrap_or("bundle"),
                    node.name
                ));
                continue;
            }
        };
        let Ok(file) = SerializedFileParser::from_bytes_with_options(node_bytes, false) else {
            continue;
        };
        files.push(parse_shape_data_from_file(
            &file,
            archive_id_from_external_path(&node.name),
        ));
    }
    Ok((ParsedBundleGeometry { files }, warnings))
}

fn is_resource_payload_node(name: &str) -> bool {
    name.to_ascii_lowercase().ends_with(".ress")
}

fn parse_shape_data_from_file(
    file: &SerializedFile,
    archive_id: Option<String>,
) -> ParsedFileGeometry {
    let mut gameobjects = HashMap::<i64, GameObjectInfo>::new();
    let mut transforms = HashMap::<i64, TransformInfo>::new();
    let mut meshes = HashMap::<i64, MeshInfo>::new();
    let mut colliders = Vec::<ColliderInfo>::new();
    let mut markers = HashSet::<String>::new();

    for object in file.object_handles() {
        if let Ok(Some(name)) = object.peek_name() {
            let marker = normalize_key(&name);
            if !marker.is_empty() {
                markers.insert(marker);
            }
        }
        let class_id = object.class_id();
        if !matches!(class_id, 1 | 4 | 43 | 64 | 65 | 135 | 136) {
            continue;
        }
        let Ok(parsed) = object.read() else {
            continue;
        };
        let class = parsed.as_unity_class();
        match class_id {
            1 => {
                let name = class
                    .get("m_Name")
                    .and_then(UnityValue::as_str)
                    .unwrap_or("")
                    .to_string();
                let active = class
                    .get("m_IsActive")
                    .and_then(UnityValue::as_bool)
                    .unwrap_or(true);
                gameobjects.insert(object.path_id(), GameObjectInfo { name, active });
            }
            4 => {
                let go_id = class.get("m_GameObject").and_then(pptr_path_id);
                let Some(go_id) = go_id else {
                    continue;
                };
                transforms.insert(
                    object.path_id(),
                    TransformInfo {
                        go_id,
                        parent_transform_id: class.get("m_Father").and_then(pptr_path_id),
                        local_position: class
                            .get("m_LocalPosition")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ZERO),
                        local_rotation: class
                            .get("m_LocalRotation")
                            .and_then(quat_value)
                            .unwrap_or(Quat::IDENTITY),
                        local_scale: class
                            .get("m_LocalScale")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ONE),
                    },
                );
            }
            43 => {
                if let Some((center, half_extents)) = class.get("m_LocalAABB").and_then(aabb_value)
                {
                    meshes.insert(
                        object.path_id(),
                        MeshInfo {
                            center,
                            half_extents,
                        },
                    );
                }
            }
            64 | 65 | 135 | 136 => {
                if !class
                    .get("m_Enabled")
                    .and_then(UnityValue::as_bool)
                    .unwrap_or(true)
                {
                    continue;
                }
                if class
                    .get("m_IsTrigger")
                    .and_then(UnityValue::as_bool)
                    .unwrap_or(false)
                {
                    continue;
                }
                let Some(go_id) = class.get("m_GameObject").and_then(pptr_path_id) else {
                    continue;
                };
                let kind = match class_id {
                    64 => ColliderKind::Mesh {
                        mesh_ref: class.get("m_Mesh").and_then(pptr_ref),
                    },
                    65 => {
                        let center = class
                            .get("m_Center")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ZERO);
                        let size = class
                            .get("m_Size")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ONE);
                        ColliderKind::Box {
                            center,
                            half_extents: size.abs().mul_scalar(0.5),
                        }
                    }
                    135 => {
                        let center = class
                            .get("m_Center")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ZERO);
                        let radius = class
                            .get("m_Radius")
                            .and_then(f32_value)
                            .unwrap_or(0.1)
                            .abs();
                        ColliderKind::Sphere { center, radius }
                    }
                    _ => {
                        let center = class
                            .get("m_Center")
                            .and_then(vec3_value)
                            .unwrap_or(Vec3::ZERO);
                        let radius = class
                            .get("m_Radius")
                            .and_then(f32_value)
                            .unwrap_or(0.1)
                            .abs();
                        let height = class
                            .get("m_Height")
                            .and_then(f32_value)
                            .unwrap_or(radius * 2.0)
                            .abs();
                        let direction = class
                            .get("m_Direction")
                            .and_then(UnityValue::as_i64)
                            .unwrap_or(1) as i32;
                        ColliderKind::Capsule {
                            center,
                            radius,
                            height,
                            direction,
                        }
                    }
                };
                colliders.push(ColliderInfo { go_id, kind });
            }
            _ => {}
        }
    }

    ParsedFileGeometry {
        archive_id,
        external_archives: file
            .externals
            .iter()
            .map(|external| archive_id_from_external_path(&external.path))
            .collect(),
        markers,
        gameobjects,
        transforms,
        meshes,
        colliders,
    }
}

fn extract_shape_candidates_from_parsed_file(
    file_key: usize,
    parsed: &ParsedFileGeometry,
    mesh_lookup: &HashMap<(usize, i64), MeshInfo>,
    archive_to_file: &HashMap<String, usize>,
) -> Vec<RawShapeCandidate> {
    let transform_by_go = parsed
        .transforms
        .iter()
        .map(|(transform_id, transform)| (transform.go_id, *transform_id))
        .collect::<HashMap<_, _>>();
    let mut memo = HashMap::new();
    let mut candidates = Vec::new();

    for collider in &parsed.colliders {
        let Some(transform_id) = transform_by_go.get(&collider.go_id).copied() else {
            continue;
        };
        if !transform_hierarchy_active(transform_id, &parsed.gameobjects, &parsed.transforms) {
            continue;
        }
        let Some(world) = world_transform(
            transform_id,
            &parsed.transforms,
            &mut memo,
            &mut HashSet::new(),
        ) else {
            continue;
        };
        let label = object_label(collider.go_id, &parsed.gameobjects);
        let path = hierarchy_path(transform_id, &parsed.gameobjects, &parsed.transforms);
        let mesh = collider.mesh_ref().and_then(|mesh_ref| {
            resolve_mesh_ref(file_key, parsed, mesh_ref, mesh_lookup, archive_to_file)
        });
        let Some((center, half_extents, shape_name)) = collider.local_aabb(mesh) else {
            continue;
        };
        push_raw_shape_candidate(
            &mut candidates,
            file_key,
            &label,
            &path,
            &shape_name,
            center,
            half_extents,
            &world,
            0.95,
        );
    }
    candidates
}

fn resolve_mesh_ref<'a>(
    current_file_key: usize,
    parsed: &ParsedFileGeometry,
    mesh_ref: PPtrRef,
    mesh_lookup: &'a HashMap<(usize, i64), MeshInfo>,
    archive_to_file: &HashMap<String, usize>,
) -> Option<&'a MeshInfo> {
    let file_key = if mesh_ref.file_id == 0 {
        current_file_key
    } else {
        let external_index = mesh_ref.file_id.checked_sub(1)? as usize;
        let archive_id = parsed.external_archives.get(external_index)?.as_ref()?;
        *archive_to_file.get(archive_id)?
    };
    mesh_lookup.get(&(file_key, mesh_ref.path_id))
}

fn append_scope_shape_candidates(
    group: &BundleGroupShapeCandidates,
    scope_kind: &str,
    scope_id: &str,
    aliases: &[String],
    doc: &mut CollisionGeometryDocument,
) {
    let before = doc.shapes.len();
    append_graph_shape_candidates_for_match(group, scope_kind, scope_id, scope_id, doc);
    if doc.shapes.len() > before {
        return;
    }
    for alias in aliases {
        append_graph_shape_candidates_for_match(group, scope_kind, alias, scope_id, doc);
        if doc.shapes.len() > before {
            return;
        }
    }
}

fn append_graph_shape_candidates_for_match(
    group: &BundleGroupShapeCandidates,
    scope_kind: &str,
    match_id: &str,
    source_id: &str,
    doc: &mut CollisionGeometryDocument,
) {
    let scene_scope = scope_kind == SCOPE_ENVIRONMENT || scope_kind == SCOPE_RACE;
    let matching_files = matching_graph_file_keys(
        scene_scope,
        match_id,
        &group.file_markers,
        &group.file_bundle_keys,
    );
    if !matching_files.iter().any(|matches| *matches) {
        return;
    }
    for candidate in &group.candidates {
        if doc.shapes.len() >= MAX_SHAPES_PER_SCOPE {
            break;
        }
        if !matching_files
            .get(candidate.file_key)
            .copied()
            .unwrap_or(false)
        {
            continue;
        }
        if should_skip_label(&candidate.label, &candidate.hierarchy_path) {
            continue;
        }
        if !scene_scope && !candidate_root_matches_normalized_scope(match_id, candidate) {
            continue;
        }
        let id = format!("{}:{}:{}", scope_kind, source_id, doc.shapes.len());
        let object_path = if candidate.hierarchy_path.is_empty() {
            None
        } else {
            Some(candidate.hierarchy_path.clone())
        };
        doc.shapes.push(CollisionShape {
            id,
            source_kind: scope_kind.into(),
            source_id: source_id.into(),
            label: if candidate.hierarchy_path.is_empty() {
                candidate.label.clone()
            } else {
                candidate.hierarchy_path.clone()
            },
            provenance: Some(
                if scene_scope {
                    PROVENANCE_SCENE_GRAPH
                } else {
                    PROVENANCE_PREFAB_GRAPH
                }
                .into(),
            ),
            source_asset: Some(match_id.into()),
            object_path,
            shape: candidate.shape_name.clone(),
            center: candidate.center.to_array(),
            half_extents: candidate.half_extents.to_array(),
            rotation: candidate.rotation.to_array(),
            confidence: candidate.confidence.clamp(0.0, 1.0),
        });
    }
}

fn push_raw_shape_candidate(
    out: &mut Vec<RawShapeCandidate>,
    file_key: usize,
    label: &str,
    hierarchy_path: &str,
    shape_name: &str,
    local_center: Vec3,
    local_half_extents: Vec3,
    world: &TransformWorld,
    confidence: f32,
) {
    if should_skip_label(label, hierarchy_path) {
        return;
    }
    let half_extents = local_half_extents.abs();
    if half_extents.max_component() <= 0.001 {
        return;
    }
    let (center, extents, rotation) =
        transform_oriented_bounds(local_center, half_extents, Quat::IDENTITY, world);
    if !center.is_finite() || !extents.is_finite() || extents.max_component() > 10_000.0 {
        return;
    }

    out.push(RawShapeCandidate {
        file_key,
        label: label.into(),
        hierarchy_path: hierarchy_path.into(),
        shape_name: shape_name.into(),
        center,
        half_extents: extents,
        rotation,
        confidence,
    });
}

pub fn load_course_geometry(
    conn: &Connection,
    course: &ReplayCourseData,
) -> AppResult<CourseCollisionGeometry> {
    let mut shapes = Vec::new();
    let mut warnings = Vec::new();
    let mut unavailable = false;

    if let Some(environment_id) = &course.environment_id {
        load_cached_scope_geometry(
            conn,
            &course.cache_id,
            SCOPE_ENVIRONMENT,
            environment_id,
            "Environment",
            true,
            &mut shapes,
            &mut warnings,
            &mut unavailable,
        )?;
    }

    if let Some(race_asset_key) = &course.race_asset_key {
        load_cached_scope_geometry(
            conn,
            &course.cache_id,
            SCOPE_RACE,
            race_asset_key,
            "Race scene",
            false,
            &mut shapes,
            &mut warnings,
            &mut unavailable,
        )?;
    }

    let props = collision_props_for_course(course);
    let unique_items = props
        .iter()
        .filter(|prop| !prop.procedural_geometry)
        .map(|prop| prop.item_id.as_str())
        .collect::<BTreeSet<_>>();
    let mut item_docs = HashMap::new();
    for item_id in unique_items {
        if let Some(row) =
            repositories::get_collision_geometry_scope(conn, &course.cache_id, SCOPE_ITEM, item_id)?
        {
            if row.status == "ready" || row.status == "partial" {
                let doc: CollisionGeometryDocument = serde_json::from_str(&row.geometry_json)?;
                warnings.extend(doc.warnings.clone());
                item_docs.insert(item_id.to_string(), doc);
            }
        }
    }

    for prop in props {
        if prop.procedural_geometry {
            shapes.extend(procedural_ribbon_shapes(&prop));
            continue;
        }
        let Some(doc) = item_docs.get(&prop.item_id) else {
            continue;
        };
        let placement = placement_transform(&prop);
        for shape in &doc.shapes {
            shapes.push(transform_cached_shape(shape, &prop, &placement));
        }
    }

    shapes.retain(shape_can_confirm_collision);

    if shapes.is_empty() {
        unavailable = true;
    }

    Ok(CourseCollisionGeometry {
        shapes,
        warnings,
        unavailable,
    })
}

fn load_cached_scope_geometry(
    conn: &Connection,
    cache_id: &str,
    scope_kind: &str,
    scope_id: &str,
    label: &str,
    required: bool,
    shapes: &mut Vec<CollisionShape>,
    warnings: &mut Vec<String>,
    unavailable: &mut bool,
) -> AppResult<()> {
    match repositories::get_collision_geometry_scope(conn, cache_id, scope_kind, scope_id)? {
        Some(row) => {
            let doc: CollisionGeometryDocument = serde_json::from_str(&row.geometry_json)?;
            if row.status == "ready" || row.status == "partial" {
                shapes.extend(doc.shapes);
            } else if required {
                *unavailable = true;
                warnings.push(format!("{label} geometry unavailable: {scope_id}"));
            }
            if required || row.status == "ready" || row.status == "partial" {
                warnings.extend(doc.warnings);
            }
        }
        None => {
            if required {
                *unavailable = true;
                warnings.push(format!("{label} geometry missing: {scope_id}"));
            }
        }
    }
    Ok(())
}

pub fn confirm_collision_events(
    events: &[CollisionEvent],
    samples: &[TelemetrySample],
    geometry: Option<&CourseCollisionGeometry>,
) -> Vec<CollisionEvent> {
    let Some(geometry) = geometry else {
        return Vec::new();
    };
    if geometry.shapes.is_empty() {
        return Vec::new();
    }
    let shapes = geometry
        .shapes
        .iter()
        .filter_map(PreparedCollisionShape::new)
        .collect::<Vec<_>>();
    if shapes.is_empty() {
        return Vec::new();
    }

    events
        .iter()
        .filter_map(|event| confirm_event(event, samples, &shapes))
        .collect()
}

fn confirm_event<'a>(
    event: &CollisionEvent,
    samples: &[TelemetrySample],
    shapes: &[PreparedCollisionShape<'a>],
) -> Option<CollisionEvent> {
    let points = event_sweep_points(event, samples);
    if points.is_empty() {
        return None;
    }
    let threshold = DRONE_RADIUS_METERS + CONTACT_MARGIN_METERS;
    let mut best: Option<(&CollisionShape, f32)> = None;

    for prepared in shapes {
        let mut min_distance = f32::INFINITY;
        for point in &points {
            if point.sub(prepared.center).length_squared() > prepared.broad_radius_squared {
                continue;
            }
            min_distance = min_distance.min(distance_point_obb(
                *point,
                prepared.center,
                prepared.half_extents,
                prepared.rotation,
            ));
            if min_distance <= threshold {
                break;
            }
        }
        if min_distance <= threshold
            && best
                .map(|(_, distance)| min_distance < distance)
                .unwrap_or(true)
        {
            best = Some((prepared.shape, min_distance));
        }
    }

    let (shape, distance) = best?;
    let mut confirmed = event.clone();
    confirmed.geometry_confirmed = true;
    confirmed.geometry_status = Some("confirmed".into());
    confirmed.hit_source = Some(format!("{}:{}", shape.source_kind, shape.source_id));
    confirmed.hit_label = Some(shape.label.clone());
    confirmed.hit_shape = Some(shape.shape.clone());
    confirmed.hit_distance = Some(distance);
    confirmed.confidence = (confirmed.confidence * 0.75 + shape.confidence * 0.25).clamp(0.0, 1.0);
    Some(confirmed)
}

fn shape_can_confirm_collision(shape: &CollisionShape) -> bool {
    if is_non_physical_collider_label(&shape.label) {
        return false;
    }
    matches!(
        shape.shape.as_str(),
        "mesh_collider"
            | "box_collider"
            | "sphere_collider"
            | "capsule_collider"
            | "procedural_ribbon_segment"
    )
}

struct PreparedCollisionShape<'a> {
    shape: &'a CollisionShape,
    center: Vec3,
    half_extents: Vec3,
    rotation: Quat,
    broad_radius_squared: f32,
}

impl<'a> PreparedCollisionShape<'a> {
    fn new(shape: &'a CollisionShape) -> Option<Self> {
        if !shape_can_confirm_collision(shape) {
            return None;
        }
        let half_extents = Vec3::from_array(shape.half_extents);
        if half_extents.max_component() <= 0.001 {
            return None;
        }
        let center = Vec3::from_array(shape.center);
        let rotation = Quat::from_array(shape.rotation);
        let broad_radius = half_extents.magnitude() + DRONE_RADIUS_METERS + CONTACT_MARGIN_METERS;

        Some(Self {
            shape,
            center,
            half_extents,
            rotation,
            broad_radius_squared: broad_radius * broad_radius,
        })
    }
}

fn is_non_physical_collider_label(label: &str) -> bool {
    let key = normalize_key(label);
    NON_PHYSICAL_COLLIDER_LABEL_PARTS
        .iter()
        .any(|needle| key.contains(needle))
}

fn event_sweep_points(event: &CollisionEvent, samples: &[TelemetrySample]) -> Vec<Vec3> {
    let event_time = event.capture_time_seconds;
    let mut points = Vec::new();
    for sample in samples {
        if sample.capture_time_seconds < event_time - 0.10
            || sample.capture_time_seconds > event_time + 0.25
        {
            continue;
        }
        if let Some(pos) = sample.position {
            points.push(Vec3::new(pos.x, pos.y, pos.z));
        }
    }
    if points.is_empty() {
        if let Some(pos) = event.pos {
            points.push(Vec3::from_array(pos));
        }
    }
    if points.len() == 2 {
        let a = points[0];
        let b = points[1];
        for i in 1..4 {
            let t = i as f32 / 4.0;
            points.push(a.lerp(b, t));
        }
    }
    points
}

fn distance_point_aabb(point: Vec3, center: Vec3, half_extents: Vec3) -> f32 {
    let d = point.sub(center).abs().sub(half_extents);
    Vec3::new(d.x.max(0.0), d.y.max(0.0), d.z.max(0.0)).magnitude()
}

fn distance_point_obb(point: Vec3, center: Vec3, half_extents: Vec3, rotation: Quat) -> f32 {
    let local = rotation.normalized().conjugate().rotate(point.sub(center));
    distance_point_aabb(local, Vec3::ZERO, half_extents)
}

fn collision_props_for_course(course: &ReplayCourseData) -> Vec<ReplayCourseProp> {
    if !course.collision_props.is_empty() {
        course.collision_props.clone()
    } else {
        course.props.clone()
    }
}

fn procedural_ribbon_shapes(prop: &ReplayCourseProp) -> Vec<CollisionShape> {
    let mut shapes = Vec::new();
    if prop.attach_points.len() < 2 {
        return shapes;
    }

    let radius = PROCEDURAL_RIBBON_RADIUS_METERS;
    let radius_extents = Vec3::new(radius, radius, radius);
    for (idx, points) in prop.attach_points.windows(2).enumerate() {
        let start = Vec3::from_array(points[0]);
        let end = Vec3::from_array(points[1]);
        let delta = end.sub(start);
        if delta.magnitude() <= 0.001 {
            continue;
        }
        let center = start.add(end).mul_scalar(0.5);
        let half_extents = delta.abs().mul_scalar(0.5).add(radius_extents);
        shapes.push(CollisionShape {
            id: format!(
                "procedural_ribbon:{}:{}:{}",
                prop.item_id, prop.instance_id, idx
            ),
            source_kind: SCOPE_ITEM.into(),
            source_id: prop.item_id.clone(),
            label: format!(
                "{} ribbon #{} segment {}",
                prop.item_id,
                prop.instance_id,
                idx + 1
            ),
            provenance: Some(PROVENANCE_TRACK_XML_PROCEDURAL.into()),
            source_asset: Some(prop.item_id.clone()),
            object_path: None,
            shape: "procedural_ribbon_segment".into(),
            center: center.to_array(),
            half_extents: half_extents.to_array(),
            rotation: Quat::IDENTITY.to_array(),
            confidence: 0.75,
        });
    }
    shapes
}

fn transform_cached_shape(
    shape: &CollisionShape,
    prop: &ReplayCourseProp,
    placement: &TransformWorld,
) -> CollisionShape {
    let center = Vec3::from_array(shape.center);
    let half_extents = Vec3::from_array(shape.half_extents);
    let rotation = Quat::from_array(shape.rotation);
    let (world_center, world_extents, world_rotation) =
        transform_oriented_bounds(center, half_extents, rotation, placement);
    let mut out = shape.clone();
    out.id = format!("{}:{}:{}", shape.id, prop.item_id, prop.instance_id);
    out.source_kind = SCOPE_ITEM.into();
    out.source_id = prop.item_id.clone();
    out.label = format!("{} #{}", shape.label, prop.instance_id);
    out.source_asset = Some(prop.item_id.clone());
    out.center = world_center.to_array();
    out.half_extents = world_extents.to_array();
    out.rotation = world_rotation.to_array();
    out
}

fn placement_transform(prop: &ReplayCourseProp) -> TransformWorld {
    TransformWorld {
        position: Vec3::from_array(prop.position),
        rotation: Quat::from_euler_zxy(prop.rotation),
        scale: Vec3::ONE,
    }
}

impl AddressablesIndex {
    fn from_data_root(data_root: &Path) -> Self {
        let aa_root = data_root.join("StreamingAssets").join("aa");
        let mut bundle_paths = Vec::new();
        collect_bundles(&aa_root, &mut bundle_paths);
        bundle_paths.sort();

        let bundles_by_name = bundle_paths
            .iter()
            .filter_map(|path| {
                path.file_name()
                    .and_then(|n| n.to_str())
                    .map(|name| (name.to_string(), path.clone()))
            })
            .collect::<HashMap<_, _>>();
        let mut catalog = Vec::new();
        let mut decoded_catalogs = Vec::new();
        let mut binary_catalogs = Vec::new();
        collect_catalog_bytes(
            &aa_root,
            &mut catalog,
            &mut decoded_catalogs,
            &mut binary_catalogs,
        );
        let bundle_positions = ascii_tokens(&catalog)
            .into_iter()
            .filter(|(_, token)| token.ends_with(".bundle"))
            .collect();

        Self {
            catalog,
            bundle_positions,
            bundle_paths,
            bundles_by_name,
            decoded_catalogs,
            binary_catalogs,
        }
    }

    fn candidate_bundles(&self, scope_id: &str, broad_fallback: bool) -> Vec<PathBuf> {
        let mut decoded = self.decoded_candidate_bundles(scope_id, broad_fallback);
        for alias in catalog_candidate_aliases(scope_id, broad_fallback) {
            extend_unique_paths(
                &mut decoded,
                self.decoded_candidate_bundles(alias, broad_fallback),
            );
        }
        if !decoded.is_empty() {
            return decoded;
        }

        if self.bundle_positions.is_empty() {
            return Vec::new();
        }

        let needle = scope_id.as_bytes();
        let mut scored = BTreeMap::<(usize, String), PathBuf>::new();
        for hit in byte_positions(&self.catalog, needle) {
            for (pos, name) in &self.bundle_positions {
                let distance = pos.abs_diff(hit);
                if distance > MAX_CATALOG_WINDOW_BYTES {
                    continue;
                }
                if let Some(path) = self.bundles_by_name.get(name) {
                    scored.insert((distance, name.clone()), path.clone());
                }
            }
        }

        scored
            .into_values()
            .take(MAX_CANDIDATE_BUNDLES)
            .collect::<Vec<_>>()
    }

    fn decoded_candidate_bundles(&self, scope_id: &str, is_environment: bool) -> Vec<PathBuf> {
        let limit = if is_environment {
            MAX_DECODED_ENVIRONMENT_CANDIDATE_BUNDLES
        } else {
            MAX_DECODED_ITEM_CANDIDATE_BUNDLES
        };
        let mut names = Vec::new();
        let mut seen_names = HashSet::new();
        for catalog in &self.decoded_catalogs {
            for name in catalog.candidate_bundle_names(scope_id, is_environment) {
                if seen_names.insert(name.clone()) {
                    names.push(name);
                }
            }
        }
        for catalog in &self.binary_catalogs {
            for name in catalog.candidate_bundle_names(scope_id, is_environment) {
                if seen_names.insert(name.clone()) {
                    names.push(name);
                }
            }
        }

        let mut seen_paths = HashSet::new();
        names
            .into_iter()
            .filter_map(|name| {
                self.bundles_by_name
                    .get(&name)
                    .or_else(|| self.bundles_by_name.get(&name.to_ascii_lowercase()))
                    .cloned()
            })
            .filter(|path| seen_paths.insert(path.clone()))
            .take(limit)
            .collect()
    }
}

impl ArchiveBundleResolver {
    fn new(index: &AddressablesIndex) -> Self {
        Self {
            bundle_paths: index.bundle_paths.clone(),
            archive_bundle_cache: HashMap::new(),
        }
    }

    fn find_bundle_for_archive(&mut self, archive_id: &str) -> Option<PathBuf> {
        let archive = archive_id.to_ascii_lowercase();
        if archive.is_empty() {
            return None;
        }
        if let Some(cached) = self.archive_bundle_cache.get(&archive) {
            return cached.clone();
        }

        let found = self
            .bundle_paths
            .iter()
            .find(|path| bundle_contains_archive(path, &archive))
            .cloned();
        self.archive_bundle_cache.insert(archive, found.clone());
        found
    }
}

fn bundle_contains_archive(path: &Path, archive_id: &str) -> bool {
    let mut options = BundleLoadOptions::lazy();
    options.max_memory = Some(MAX_NODE_BYTES as usize);
    options.max_unityfs_block_cache_memory = Some(MAX_LAZY_BLOCK_CACHE_BYTES);
    options.max_compressed_block_size = Some(MAX_NODE_BYTES as usize);

    let Ok(bundle) = load_bundle_file_with_options(path, options) else {
        return false;
    };
    if bundle
        .asset_names
        .iter()
        .any(|name| archive_name_matches(name, archive_id))
    {
        return true;
    }
    bundle
        .node_names()
        .into_iter()
        .any(|name| archive_name_matches(name, archive_id))
}

fn archive_name_matches(name: &str, archive_id: &str) -> bool {
    let normalized = name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or(name)
        .trim_end_matches(".resS")
        .to_ascii_lowercase();
    normalized == archive_id
}

fn extend_unique_paths(out: &mut Vec<PathBuf>, paths: Vec<PathBuf>) {
    let mut seen = out.iter().cloned().collect::<HashSet<_>>();
    for path in paths {
        if seen.insert(path.clone()) {
            out.push(path);
        }
    }
}

fn catalog_candidate_aliases(scope_id: &str, is_environment: bool) -> Vec<&'static str> {
    let scope = normalize_key(scope_id);
    if is_environment {
        return match scope.as_str() {
            // Liftoff Micro Drones stores the Sawdust Inc track layouts under
            // SawdustInc IDs, while the reusable scene geometry is addressed as
            // WoodWorkshop.
            "sawdustinc" => vec!["WoodWorkshop"],
            "sawdustincnight" => vec!["WoodWorkshop_Night"],
            _ => Vec::new(),
        };
    }

    if scope.starts_with("airflagliftofffinish") {
        return vec!["Airflagv01StartFinish01", "AirFlagv01", "AirFlagv01_Baked"];
    }
    if scope.starts_with("airflagliftoffslalom") {
        return vec!["Airflagv01Slalom01", "AirFlagv01", "AirFlagv01_Baked"];
    }
    if scope.starts_with("airflagliftoffturn") {
        return vec!["Airflagv01Turn01", "AirFlagv01", "AirFlagv01_Baked"];
    }
    if scope.starts_with("banner5x5mliftoff") {
        return vec!["Banner5x5m01"];
    }
    match scope.as_str() {
        "constructionfencingliftoffblack01" => vec!["HerasFencing01"],
        _ => Vec::new(),
    }
}

impl DecodedAddressablesCatalog {
    fn from_json_bytes(bytes: &[u8]) -> Option<Self> {
        let raw: AddressablesCatalogJson = serde_json::from_slice(bytes).ok()?;
        let key_bytes = general_purpose::STANDARD.decode(raw.key_data).ok()?;
        let bucket_bytes = general_purpose::STANDARD.decode(raw.bucket_data).ok()?;
        let entry_bytes = general_purpose::STANDARD.decode(raw.entry_data).ok()?;
        let keys = parse_catalog_keys(&key_bytes)?;
        let buckets = parse_catalog_buckets(&bucket_bytes)?;
        let entries = parse_catalog_entries(&entry_bytes)?;

        let (normalized_keys, key_indices_by_normalized_value) = normalized_catalog_keys(&keys);

        Some(Self {
            keys,
            normalized_keys,
            key_indices_by_normalized_value,
            buckets,
            entries,
            internal_ids: raw.internal_ids,
        })
    }

    fn candidate_bundle_names(&self, scope_id: &str, is_environment: bool) -> Vec<String> {
        let hits = if is_environment {
            environment_key_hits(&self.keys, &self.normalized_keys, scope_id)
        } else {
            item_key_hits(
                &self.keys,
                &self.normalized_keys,
                &self.key_indices_by_normalized_value,
                scope_id,
            )
        };
        let mut out = Vec::new();
        let mut seen = HashSet::new();
        for hit in hits {
            self.append_bundle_names_for_key(hit.index, &mut out, &mut seen);
        }
        out
    }

    fn append_bundle_names_for_key(
        &self,
        key_index: usize,
        out: &mut Vec<String>,
        seen_names: &mut HashSet<String>,
    ) {
        let mut seen_entries = HashSet::new();
        let mut seen_keys = HashSet::new();
        self.append_bundle_names_for_key_inner(
            key_index,
            out,
            seen_names,
            &mut seen_entries,
            &mut seen_keys,
            0,
        );
    }

    fn append_bundle_names_for_key_inner(
        &self,
        key_index: usize,
        out: &mut Vec<String>,
        seen_names: &mut HashSet<String>,
        seen_entries: &mut HashSet<usize>,
        seen_keys: &mut HashSet<usize>,
        depth: usize,
    ) {
        if depth > 4 || !seen_keys.insert(key_index) {
            return;
        }
        let Some(entry_indices) = self.buckets.get(key_index) else {
            return;
        };
        for entry_index in entry_indices {
            self.append_bundle_names_for_entry(
                *entry_index,
                out,
                seen_names,
                seen_entries,
                seen_keys,
                depth,
            );
        }
    }

    fn append_bundle_names_for_entry(
        &self,
        entry_index: usize,
        out: &mut Vec<String>,
        seen_names: &mut HashSet<String>,
        seen_entries: &mut HashSet<usize>,
        seen_keys: &mut HashSet<usize>,
        depth: usize,
    ) {
        if !seen_entries.insert(entry_index) {
            return;
        }
        let Some(entry) = self.entries.get(entry_index) else {
            return;
        };
        if let Some(internal_id) =
            non_negative_index(entry[0]).and_then(|idx| self.internal_ids.get(idx))
        {
            if let Some(name) = catalog_bundle_name(internal_id) {
                if seen_names.insert(name.clone()) {
                    out.push(name);
                }
            }
        }

        let Some(dependency_key_index) = non_negative_index(entry[2]) else {
            return;
        };
        self.append_bundle_names_for_key_inner(
            dependency_key_index,
            out,
            seen_names,
            seen_entries,
            seen_keys,
            depth + 1,
        );
    }
}

impl BinaryAddressablesCatalog {
    fn from_bytes(bytes: Vec<u8>) -> Option<Self> {
        const BINARY_CATALOG_MAGIC: u32 = 0x0de3_8942;
        let magic = read_u32_at(&bytes, 0)?;
        if magic != BINARY_CATALOG_MAGIC {
            return None;
        }
        let version = read_u32_at(&bytes, 4)?;
        if version > 2 {
            return None;
        }

        let keys_offset = read_u32_at(&bytes, 8)?;
        let key_data = read_binary_key_data_array(&bytes, keys_offset)?;
        let mut keys = Vec::with_capacity(key_data.len());
        let mut location_set_offsets = Vec::with_capacity(key_data.len());
        for (key_name_offset, location_set_offset) in key_data {
            keys.push(read_binary_object_string(&bytes, key_name_offset));
            location_set_offsets.push(location_set_offset);
        }
        let (normalized_keys, key_indices_by_normalized_value) = normalized_catalog_keys(&keys);

        Some(Self {
            bytes,
            keys,
            normalized_keys,
            key_indices_by_normalized_value,
            location_set_offsets,
        })
    }

    fn candidate_bundle_names(&self, scope_id: &str, is_environment: bool) -> Vec<String> {
        let hits = if is_environment {
            environment_key_hits(&self.keys, &self.normalized_keys, scope_id)
        } else {
            item_key_hits(
                &self.keys,
                &self.normalized_keys,
                &self.key_indices_by_normalized_value,
                scope_id,
            )
        };
        let mut out = Vec::new();
        let mut seen_names = HashSet::new();
        let mut seen_locations = HashSet::new();
        for hit in hits {
            self.append_bundle_names_for_key(
                hit.index,
                &mut out,
                &mut seen_names,
                &mut seen_locations,
            );
        }
        out
    }

    fn append_bundle_names_for_key(
        &self,
        key_index: usize,
        out: &mut Vec<String>,
        seen_names: &mut HashSet<String>,
        seen_locations: &mut HashSet<u32>,
    ) {
        let Some(location_set_offset) = self.location_set_offsets.get(key_index) else {
            return;
        };
        let Some(location_offsets) = read_binary_u32_array(&self.bytes, *location_set_offset)
        else {
            return;
        };
        for location_offset in location_offsets {
            self.append_bundle_names_for_location(
                location_offset,
                out,
                seen_names,
                seen_locations,
                0,
            );
        }
    }

    fn append_bundle_names_for_location(
        &self,
        location_offset: u32,
        out: &mut Vec<String>,
        seen_names: &mut HashSet<String>,
        seen_locations: &mut HashSet<u32>,
        depth: usize,
    ) {
        if depth > 8 || !seen_locations.insert(location_offset) {
            return;
        }
        let Some(location) = read_binary_location_data(&self.bytes, location_offset) else {
            return;
        };
        if let Some(internal_id) =
            read_binary_string(&self.bytes, location.internal_id_offset, '/', true)
        {
            if let Some(name) = catalog_bundle_name(&internal_id) {
                if seen_names.insert(name.clone()) {
                    out.push(name);
                }
            }
        }

        if location.dependency_set_offset == u32::MAX {
            return;
        }
        let Some(dependencies) = read_binary_u32_array(&self.bytes, location.dependency_set_offset)
        else {
            return;
        };
        for dependency_offset in dependencies {
            self.append_bundle_names_for_location(
                dependency_offset,
                out,
                seen_names,
                seen_locations,
                depth + 1,
            );
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct BinaryLocationData {
    internal_id_offset: u32,
    dependency_set_offset: u32,
}

fn normalized_catalog_keys(
    keys: &[Option<String>],
) -> (Vec<Option<String>>, HashMap<String, Vec<usize>>) {
    let normalized_keys = keys
        .iter()
        .map(|key| key.as_ref().map(|key| normalize_key(key)))
        .collect::<Vec<_>>();
    let mut key_indices_by_normalized_value = HashMap::<String, Vec<usize>>::new();
    for (idx, key) in normalized_keys.iter().enumerate() {
        let Some(key) = key else {
            continue;
        };
        if key.is_empty() {
            continue;
        }
        key_indices_by_normalized_value
            .entry(key.clone())
            .or_default()
            .push(idx);
    }
    (normalized_keys, key_indices_by_normalized_value)
}

fn item_key_hits(
    keys: &[Option<String>],
    normalized_keys: &[Option<String>],
    key_indices_by_normalized_value: &HashMap<String, Vec<usize>>,
    scope_id: &str,
) -> Vec<CatalogKeyHit> {
    let scope = normalize_key(scope_id);
    if scope.is_empty() {
        return Vec::new();
    }

    let mut hits = Vec::new();
    if let Some(indices) = key_indices_by_normalized_value.get(&scope) {
        for idx in indices {
            hits.push(CatalogKeyHit {
                rank: 0,
                key_len: catalog_key_len(keys, *idx),
                index: *idx,
            });
        }
        return dedup_catalog_hits(hits);
    }

    for (idx, key) in normalized_keys.iter().enumerate() {
        let Some(key) = key else {
            continue;
        };
        let Some(raw_key) = keys.get(idx).and_then(Option::as_deref) else {
            continue;
        };
        let basename = raw_key
            .rsplit(['/', '\\'])
            .next()
            .map(normalize_key)
            .unwrap_or_default();
        let rank = if basename == scope {
            1
        } else if key.contains(&scope) {
            2
        } else if key.len() >= 8 && scope.contains(key) {
            3
        } else {
            continue;
        };
        hits.push(CatalogKeyHit {
            rank,
            key_len: raw_key.len(),
            index: idx,
        });
    }
    keep_best_rank(dedup_catalog_hits(hits))
}

fn environment_key_hits(
    keys: &[Option<String>],
    normalized_keys: &[Option<String>],
    scope_id: &str,
) -> Vec<CatalogKeyHit> {
    let scope = normalize_key(scope_id);
    if scope.is_empty() {
        return Vec::new();
    }
    let exact_scene = format!("scenesenvironments{scope}{scope}unity");
    let scene_tail = format!("{scope}unity");
    let mut scene_hits = Vec::new();
    let mut exact_variant_fallback_hits = Vec::new();
    let mut exact_descriptor_hits = Vec::new();
    let mut broad_hits = Vec::new();
    let exact_empty_variant = format!("{scope}empty");
    let exact_day_variant = format!("{scope}day");
    let scope_is_scene_variant =
        scope.ends_with("empty") || scope.ends_with("night") || scope.ends_with("day");

    for (idx, key) in normalized_keys.iter().enumerate() {
        let Some(key) = key else {
            continue;
        };
        let Some(raw_key) = keys.get(idx).and_then(Option::as_deref) else {
            continue;
        };
        let lower_key = raw_key.to_ascii_lowercase();
        let is_scene_asset = lower_key.ends_with(".unity");
        let in_environment_tree = lower_key.contains("/scenes/environments/")
            || lower_key.contains("\\scenes\\environments\\");

        if key.contains(&scope) {
            let rank = if is_scene_asset && key.contains(&exact_scene) {
                0
            } else if is_scene_asset && in_environment_tree && key.ends_with(&scene_tail) {
                1
            } else if is_scene_asset
                && in_environment_tree
                && key.ends_with(&format!("{scope}dayunity"))
            {
                2
            } else if is_scene_asset
                && in_environment_tree
                && key.ends_with(&format!("{scope}emptyunity"))
            {
                4
            } else if is_scene_asset && in_environment_tree {
                3
            } else if in_environment_tree {
                5
            } else {
                6
            };
            if is_scene_asset {
                scene_hits.push(CatalogKeyHit {
                    rank,
                    key_len: raw_key.len(),
                    index: idx,
                });
            } else {
                broad_hits.push(CatalogKeyHit {
                    rank,
                    key_len: raw_key.len(),
                    index: idx,
                });
            }
        }

        // Bare environment IDs resolve to the default variant; `_Empty` is only
        // preferred when the track explicitly requests that variant.
        if scope_is_scene_variant && *key == scope {
            exact_variant_fallback_hits.push(CatalogKeyHit {
                rank: 0,
                key_len: raw_key.len(),
                index: idx,
            });
            continue;
        }

        if *key == scope {
            exact_descriptor_hits.push(CatalogKeyHit {
                rank: 0,
                key_len: raw_key.len(),
                index: idx,
            });
            continue;
        }

        if !scope_is_scene_variant {
            let rank = if *key == exact_day_variant {
                1
            } else if *key == exact_empty_variant {
                2
            } else {
                continue;
            };
            exact_variant_fallback_hits.push(CatalogKeyHit {
                rank,
                key_len: raw_key.len(),
                index: idx,
            });
        }
    }

    let scene_hits = keep_best_rank(dedup_catalog_hits(scene_hits));
    if !scene_hits.is_empty() && scene_hits[0].rank <= 2 {
        return scene_hits;
    }

    let exact_descriptor_hits = keep_best_rank(dedup_catalog_hits(exact_descriptor_hits));
    if !exact_descriptor_hits.is_empty() {
        return exact_descriptor_hits;
    }

    let exact_variant_fallback_hits =
        keep_best_rank(dedup_catalog_hits(exact_variant_fallback_hits));
    if !exact_variant_fallback_hits.is_empty() {
        return exact_variant_fallback_hits;
    }

    if !scene_hits.is_empty() {
        return scene_hits;
    }

    keep_best_rank(dedup_catalog_hits(broad_hits))
}

fn catalog_key_len(keys: &[Option<String>], index: usize) -> usize {
    keys.get(index)
        .and_then(Option::as_deref)
        .map(str::len)
        .unwrap_or(usize::MAX)
}

fn dedup_catalog_hits(mut hits: Vec<CatalogKeyHit>) -> Vec<CatalogKeyHit> {
    hits.sort();
    let mut seen = HashSet::new();
    hits.into_iter()
        .filter(|hit| seen.insert(hit.index))
        .collect()
}

fn keep_best_rank(mut hits: Vec<CatalogKeyHit>) -> Vec<CatalogKeyHit> {
    hits.sort();
    let Some(best_rank) = hits.first().map(|hit| hit.rank) else {
        return hits;
    };
    hits.into_iter()
        .filter(|hit| hit.rank == best_rank)
        .collect()
}

fn catalog_bundle_name(internal_id: &str) -> Option<String> {
    let lower = internal_id.to_ascii_lowercase();
    let end = lower.find(".bundle")? + ".bundle".len();
    let bundle_path = &internal_id[..end];
    let name = bundle_path
        .rsplit(['/', '\\'])
        .next()
        .filter(|name| !name.is_empty())?;
    Some(name.to_string())
}

fn non_negative_index(value: i32) -> Option<usize> {
    if value >= 0 {
        Some(value as usize)
    } else {
        None
    }
}

fn read_binary_key_data_array(bytes: &[u8], id: u32) -> Option<Vec<(u32, u32)>> {
    let values = read_binary_u32_array(bytes, id)?;
    if values.len() % 2 != 0 {
        return None;
    }
    Some(
        values
            .chunks_exact(2)
            .map(|chunk| (chunk[0], chunk[1]))
            .collect(),
    )
}

fn read_binary_u32_array(bytes: &[u8], id: u32) -> Option<Vec<u32>> {
    if id == u32::MAX {
        return Some(Vec::new());
    }
    let offset = id as usize;
    if offset < 4 || offset > bytes.len() {
        return None;
    }
    let size = read_u32_at(bytes, offset - 4)? as usize;
    if size % 4 != 0 {
        return None;
    }
    let end = offset.checked_add(size)?;
    if end > bytes.len() {
        return None;
    }
    let mut values = Vec::with_capacity(size / 4);
    let mut cursor = offset;
    while cursor < end {
        values.push(read_u32_at(bytes, cursor)?);
        cursor += 4;
    }
    Some(values)
}

fn read_binary_location_data(bytes: &[u8], offset: u32) -> Option<BinaryLocationData> {
    let offset = offset as usize;
    if offset.checked_add(28)? > bytes.len() {
        return None;
    }
    Some(BinaryLocationData {
        internal_id_offset: read_u32_at(bytes, offset + 4)?,
        dependency_set_offset: read_u32_at(bytes, offset + 12)?,
    })
}

fn read_binary_object_string(bytes: &[u8], offset: u32) -> Option<String> {
    if offset == u32::MAX {
        return None;
    }
    let offset = offset as usize;
    let type_id = read_u32_at(bytes, offset)?;
    let object_id = read_u32_at(bytes, offset + 4)?;
    let class_name = read_binary_type_class_name(bytes, type_id)?;
    if class_name != "System.String" {
        return None;
    }
    let object_id = object_id as usize;
    let string_id = read_u32_at(bytes, object_id)?;
    let separator = read_u16_at(bytes, object_id + 4)?;
    let separator = char::from_u32(separator as u32).unwrap_or('\0');
    read_binary_string(bytes, string_id, separator, false)
}

fn read_binary_type_class_name(bytes: &[u8], type_id: u32) -> Option<String> {
    if type_id == u32::MAX {
        return None;
    }
    let type_id = type_id as usize;
    let class_id = read_u32_at(bytes, type_id + 4)?;
    read_binary_string(bytes, class_id, '.', true)
}

fn read_binary_string(
    bytes: &[u8],
    id: u32,
    separator: char,
    dynamic_allowed: bool,
) -> Option<String> {
    const DYNAMIC_STRING_FLAG: u32 = 0x4000_0000;
    const CLEAR_FLAGS_MASK: u32 = 0x3fff_ffff;

    if id == u32::MAX {
        return None;
    }
    if separator != '\0' && dynamic_allowed && (id & DYNAMIC_STRING_FLAG) == DYNAMIC_STRING_FLAG {
        let mut parts = Vec::new();
        let mut next_id = id;
        let mut guard = 0usize;
        while next_id != u32::MAX {
            guard += 1;
            if guard > 512 {
                return None;
            }
            let offset = (next_id & CLEAR_FLAGS_MASK) as usize;
            let string_id = read_u32_at(bytes, offset)?;
            next_id = read_u32_at(bytes, offset + 4)?;
            parts.push(read_binary_string(bytes, string_id, '\0', false)?);
        }
        parts.reverse();
        return Some(parts.join(&separator.to_string()));
    }

    read_binary_auto_string(bytes, id)
}

fn read_binary_auto_string(bytes: &[u8], id: u32) -> Option<String> {
    const UNICODE_STRING_FLAG: u32 = 0x8000_0000;
    const CLEAR_FLAGS_MASK: u32 = 0x3fff_ffff;

    let is_unicode = (id & UNICODE_STRING_FLAG) == UNICODE_STRING_FLAG;
    let offset = (id & CLEAR_FLAGS_MASK) as usize;
    if offset < 4 || offset > bytes.len() {
        return None;
    }
    let len = read_u32_at(bytes, offset - 4)? as usize;
    let end = offset.checked_add(len)?;
    let data = bytes.get(offset..end)?;
    if is_unicode {
        if data.len() % 2 != 0 {
            return None;
        }
        let units = data
            .chunks_exact(2)
            .map(|chunk| u16::from_le_bytes([chunk[0], chunk[1]]));
        return char::decode_utf16(units)
            .collect::<Result<String, _>>()
            .ok();
    }
    std::str::from_utf8(data).map(str::to_string).ok()
}

fn parse_catalog_keys(bytes: &[u8]) -> Option<Vec<Option<String>>> {
    let mut offset = 0usize;
    let count = read_u32_le(bytes, &mut offset)? as usize;
    let mut keys = vec![None; count];
    for key in &mut keys {
        let Some(kind) = read_u8(bytes, &mut offset) else {
            break;
        };
        if kind != 0 {
            break;
        }
        let len = read_u32_le(bytes, &mut offset)? as usize;
        let end = offset.checked_add(len)?;
        let text_bytes = bytes.get(offset..end)?;
        *key = Some(std::str::from_utf8(text_bytes).ok()?.to_string());
        offset = end;
    }
    Some(keys)
}

fn parse_catalog_buckets(bytes: &[u8]) -> Option<Vec<Vec<usize>>> {
    let mut offset = 0usize;
    let count = read_u32_le(bytes, &mut offset)? as usize;
    let mut buckets = Vec::with_capacity(count);
    for _ in 0..count {
        let _key_data_offset = read_i32_le(bytes, &mut offset)?;
        let entry_count = read_i32_le(bytes, &mut offset)?;
        let entry_count = non_negative_index(entry_count)?;
        if entry_count > bytes.len().saturating_sub(offset) / 4 {
            return None;
        }
        let mut bucket = Vec::with_capacity(entry_count);
        for _ in 0..entry_count {
            let entry_index = read_i32_le(bytes, &mut offset)?;
            if let Some(entry_index) = non_negative_index(entry_index) {
                bucket.push(entry_index);
            }
        }
        buckets.push(bucket);
    }
    Some(buckets)
}

fn parse_catalog_entries(bytes: &[u8]) -> Option<Vec<[i32; 7]>> {
    let mut offset = 0usize;
    let count = read_u32_le(bytes, &mut offset)? as usize;
    let required = count.checked_mul(7)?.checked_mul(4)?;
    if bytes.len().saturating_sub(offset) < required {
        return None;
    }

    let mut entries = Vec::with_capacity(count);
    for _ in 0..count {
        let mut entry = [0i32; 7];
        for value in &mut entry {
            *value = read_i32_le(bytes, &mut offset)?;
        }
        entries.push(entry);
    }
    Some(entries)
}

fn read_u8(bytes: &[u8], offset: &mut usize) -> Option<u8> {
    let value = *bytes.get(*offset)?;
    *offset += 1;
    Some(value)
}

fn read_u16_at(bytes: &[u8], offset: usize) -> Option<u16> {
    let end = offset.checked_add(2)?;
    Some(u16::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32_at(bytes: &[u8], offset: usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    Some(u32::from_le_bytes(bytes.get(offset..end)?.try_into().ok()?))
}

fn read_u32_le(bytes: &[u8], offset: &mut usize) -> Option<u32> {
    let end = offset.checked_add(4)?;
    let value = u32::from_le_bytes(bytes.get(*offset..end)?.try_into().ok()?);
    *offset = end;
    Some(value)
}

fn read_i32_le(bytes: &[u8], offset: &mut usize) -> Option<i32> {
    let end = offset.checked_add(4)?;
    let value = i32::from_le_bytes(bytes.get(*offset..end)?.try_into().ok()?);
    *offset = end;
    Some(value)
}

fn collect_bundles(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_bundles(&path, out);
        } else if path
            .file_name()
            .and_then(|n| n.to_str())
            .map(|name| name.ends_with(".bundle"))
            .unwrap_or(false)
        {
            out.push(path);
        }
    }
}

fn collect_catalog_bytes(
    dir: &Path,
    out: &mut Vec<u8>,
    decoded_catalogs: &mut Vec<DecodedAddressablesCatalog>,
    binary_catalogs: &mut Vec<BinaryAddressablesCatalog>,
) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_catalog_bytes(&path, out, decoded_catalogs, binary_catalogs);
            continue;
        }
        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name.starts_with("catalog") && (name.ends_with(".bin") || name.ends_with(".json")) {
            if let Ok(bytes) = fs::read(&path) {
                if name.ends_with(".json") {
                    if let Some(catalog) = DecodedAddressablesCatalog::from_json_bytes(&bytes) {
                        decoded_catalogs.push(catalog);
                    }
                }
                if name.ends_with(".bin") {
                    if let Some(catalog) = BinaryAddressablesCatalog::from_bytes(bytes.clone()) {
                        binary_catalogs.push(catalog);
                    }
                }
                out.extend_from_slice(&bytes);
                out.push(0);
            }
        }
    }
}

fn ascii_tokens(bytes: &[u8]) -> Vec<(usize, String)> {
    let mut out = Vec::new();
    let mut start = None;
    for (idx, byte) in bytes.iter().copied().enumerate() {
        let printable = byte.is_ascii_graphic() || byte == b' ';
        match (start, printable) {
            (None, true) => start = Some(idx),
            (Some(s), false) => {
                if idx - s >= 4 {
                    out.push((s, String::from_utf8_lossy(&bytes[s..idx]).to_string()));
                }
                start = None;
            }
            _ => {}
        }
    }
    if let Some(s) = start {
        if bytes.len() - s >= 4 {
            out.push((s, String::from_utf8_lossy(&bytes[s..]).to_string()));
        }
    }
    out
}

fn byte_positions(haystack: &[u8], needle: &[u8]) -> Vec<usize> {
    if needle.is_empty() || haystack.len() < needle.len() {
        return Vec::new();
    }
    let mut out = Vec::new();
    let mut idx = 0;
    while idx + needle.len() <= haystack.len() {
        if &haystack[idx..idx + needle.len()] == needle {
            out.push(idx);
            idx += needle.len();
        } else {
            idx += 1;
        }
    }
    out
}

fn bundle_metadata_hash(path: &Path) -> String {
    let metadata = fs::metadata(path).ok();
    let modified = metadata
        .as_ref()
        .and_then(|m| m.modified().ok())
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis())
        .unwrap_or(0);
    let size = metadata.map(|m| m.len()).unwrap_or(0);
    let payload = format!("{}:{}:{}", path.display(), size, modified);
    blake3::hash(payload.as_bytes()).to_hex().to_string()
}

fn bundle_label(path: &Path) -> String {
    path.file_name()
        .and_then(|n| n.to_str())
        .map(str::to_string)
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[derive(Debug, Clone)]
struct GameObjectInfo {
    name: String,
    active: bool,
}

#[derive(Debug, Clone, Copy)]
struct TransformInfo {
    go_id: i64,
    parent_transform_id: Option<i64>,
    local_position: Vec3,
    local_rotation: Quat,
    local_scale: Vec3,
}

#[derive(Debug, Clone, Copy)]
struct TransformWorld {
    position: Vec3,
    rotation: Quat,
    scale: Vec3,
}

#[derive(Debug, Clone)]
struct ParsedBundleGeometry {
    files: Vec<ParsedFileGeometry>,
}

#[derive(Debug, Clone)]
struct ParsedFileGeometry {
    archive_id: Option<String>,
    external_archives: Vec<Option<String>>,
    markers: HashSet<String>,
    gameobjects: HashMap<i64, GameObjectInfo>,
    transforms: HashMap<i64, TransformInfo>,
    meshes: HashMap<i64, MeshInfo>,
    colliders: Vec<ColliderInfo>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
struct PPtrRef {
    file_id: i32,
    path_id: i64,
}

#[derive(Debug, Clone)]
struct MeshInfo {
    center: Vec3,
    half_extents: Vec3,
}

#[derive(Debug, Clone, Copy)]
struct ColliderInfo {
    go_id: i64,
    kind: ColliderKind,
}

#[derive(Debug, Clone, Copy)]
enum ColliderKind {
    Mesh {
        mesh_ref: Option<PPtrRef>,
    },
    Box {
        center: Vec3,
        half_extents: Vec3,
    },
    Sphere {
        center: Vec3,
        radius: f32,
    },
    Capsule {
        center: Vec3,
        radius: f32,
        height: f32,
        direction: i32,
    },
}

impl ColliderInfo {
    fn mesh_ref(&self) -> Option<PPtrRef> {
        match self.kind {
            ColliderKind::Mesh { mesh_ref } => mesh_ref,
            _ => None,
        }
    }

    fn local_aabb(&self, mesh: Option<&MeshInfo>) -> Option<(Vec3, Vec3, String)> {
        match self.kind {
            ColliderKind::Mesh { .. } => {
                let mesh = mesh?;
                Some((mesh.center, mesh.half_extents, "mesh_collider".into()))
            }
            ColliderKind::Box {
                center,
                half_extents,
            } => Some((center, half_extents, "box_collider".into())),
            ColliderKind::Sphere { center, radius } => Some((
                center,
                Vec3::new(radius, radius, radius),
                "sphere_collider".into(),
            )),
            ColliderKind::Capsule {
                center,
                radius,
                height,
                direction,
            } => {
                let mut extents = Vec3::new(radius, radius, radius);
                let axis = (height * 0.5).max(radius);
                match direction {
                    0 => extents.x = axis,
                    2 => extents.z = axis,
                    _ => extents.y = axis,
                }
                Some((center, extents, "capsule_collider".into()))
            }
        }
    }
}

fn world_transform(
    transform_id: i64,
    transforms: &HashMap<i64, TransformInfo>,
    memo: &mut HashMap<i64, TransformWorld>,
    visiting: &mut HashSet<i64>,
) -> Option<TransformWorld> {
    if let Some(world) = memo.get(&transform_id) {
        return Some(*world);
    }
    if !visiting.insert(transform_id) {
        return None;
    }
    let local = transforms.get(&transform_id)?;
    let parent = local
        .parent_transform_id
        .and_then(|id| world_transform(id, transforms, memo, visiting));
    let world = if let Some(parent) = parent {
        let scaled = local.local_position.mul(parent.scale);
        TransformWorld {
            position: parent.position.add(parent.rotation.rotate(scaled)),
            rotation: parent.rotation.mul(local.local_rotation).normalized(),
            scale: parent.scale.mul(local.local_scale),
        }
    } else {
        TransformWorld {
            position: local.local_position,
            rotation: local.local_rotation.normalized(),
            scale: local.local_scale,
        }
    };
    visiting.remove(&transform_id);
    memo.insert(transform_id, world);
    Some(world)
}

fn transform_oriented_bounds(
    center: Vec3,
    half_extents: Vec3,
    rotation: Quat,
    transform: &TransformWorld,
) -> (Vec3, Vec3, Quat) {
    let world_center = transform
        .position
        .add(transform.rotation.rotate(center.mul(transform.scale)));
    let world_extents = half_extents.mul(transform.scale.abs()).abs();
    let world_rotation = transform.rotation.mul(rotation).normalized();
    (world_center, world_extents, world_rotation)
}

fn object_label(go_id: i64, gameobjects: &HashMap<i64, GameObjectInfo>) -> String {
    gameobjects
        .get(&go_id)
        .map(|go| go.name.clone())
        .unwrap_or_else(|| format!("GameObject {go_id}"))
}

fn transform_hierarchy_active(
    transform_id: i64,
    gameobjects: &HashMap<i64, GameObjectInfo>,
    transforms: &HashMap<i64, TransformInfo>,
) -> bool {
    let mut current = Some(transform_id);
    let mut guard = 0usize;
    while let Some(id) = current {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(transform) = transforms.get(&id) else {
            break;
        };
        if gameobjects
            .get(&transform.go_id)
            .map(|go| !go.active)
            .unwrap_or(false)
        {
            return false;
        }
        current = transform.parent_transform_id;
    }
    true
}

fn hierarchy_path(
    transform_id: i64,
    gameobjects: &HashMap<i64, GameObjectInfo>,
    transforms: &HashMap<i64, TransformInfo>,
) -> String {
    let mut names = Vec::new();
    let mut current = Some(transform_id);
    let mut guard = 0usize;
    while let Some(id) = current {
        guard += 1;
        if guard > 64 {
            break;
        }
        let Some(transform) = transforms.get(&id) else {
            break;
        };
        names.push(object_label(transform.go_id, gameobjects));
        current = transform.parent_transform_id;
    }
    names.reverse();
    names.join("/")
}

fn matching_graph_file_keys(
    scene_scope: bool,
    scope_id: &str,
    file_markers: &[HashSet<String>],
    file_bundle_keys: &[usize],
) -> Vec<bool> {
    let scope = normalize_key(scope_id);
    if scope.is_empty() {
        return vec![false; file_markers.len()];
    }
    if scene_scope {
        let mut matching_bundle_keys = HashSet::new();
        for (file_key, markers) in file_markers.iter().enumerate() {
            if scene_file_markers_match_normalized_scope(&scope, Some(markers)) {
                matching_bundle_keys
                    .insert(file_bundle_keys.get(file_key).copied().unwrap_or(file_key));
            }
        }
        return (0..file_markers.len())
            .map(|file_key| {
                let bundle_key = file_bundle_keys.get(file_key).copied().unwrap_or(file_key);
                matching_bundle_keys.contains(&bundle_key)
            })
            .collect();
    }
    file_markers
        .iter()
        .map(|markers| prefab_file_markers_match_normalized_scope(&scope, Some(markers)))
        .collect()
}

fn scene_file_markers_match_normalized_scope(
    scope: &str,
    file_markers: Option<&HashSet<String>>,
) -> bool {
    if scope.is_empty() {
        return false;
    }
    file_markers
        .into_iter()
        .flatten()
        .any(|marker| marker == scope || marker.contains(scope))
}

fn prefab_file_markers_match_normalized_scope(
    scope: &str,
    file_markers: Option<&HashSet<String>>,
) -> bool {
    if scope.is_empty() {
        return false;
    }
    file_markers
        .into_iter()
        .flatten()
        .any(|marker| marker == &scope)
}

fn candidate_root_matches_normalized_scope(scope_id: &str, candidate: &RawShapeCandidate) -> bool {
    let scope = normalize_key(scope_id);
    if scope.is_empty() {
        return false;
    }
    candidate
        .hierarchy_path
        .split('/')
        .next()
        .filter(|root| !root.trim().is_empty())
        .or_else(|| {
            candidate
                .label
                .split('/')
                .next()
                .filter(|root| !root.trim().is_empty())
        })
        .map(normalize_key)
        .is_some_and(|root| root == scope)
}

fn should_skip_label(label: &str, path: &str) -> bool {
    let key = normalize_key(&format!("{label} {path}"));
    if NON_PHYSICAL_COLLIDER_LABEL_PARTS
        .iter()
        .any(|needle| key.contains(needle))
    {
        return true;
    }
    [
        "snappoint",
        "trackeditor",
        "gizmo",
        "fogvolume",
        "checkpoint",
        "trigger",
        "guide",
    ]
    .iter()
    .any(|needle| key.contains(needle))
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn pptr_path_id(value: &UnityValue) -> Option<i64> {
    let obj = value.as_object()?;
    for key in ["m_PathID", "pathID", "path_id", "PathID"] {
        if let Some(id) = obj.get(key).and_then(UnityValue::as_i64) {
            if id != 0 {
                return Some(id);
            }
        }
    }
    None
}

fn pptr_ref(value: &UnityValue) -> Option<PPtrRef> {
    let obj = value.as_object()?;
    let mut file_id = 0i32;
    for key in ["m_FileID", "fileID", "file_id", "FileID"] {
        if let Some(id) = obj.get(key).and_then(UnityValue::as_i64) {
            file_id = id.try_into().ok()?;
            break;
        }
    }
    for key in ["m_PathID", "pathID", "path_id", "PathID"] {
        if let Some(path_id) = obj.get(key).and_then(UnityValue::as_i64) {
            if path_id != 0 {
                return Some(PPtrRef { file_id, path_id });
            }
        }
    }
    None
}

fn archive_id_from_external_path(path: &str) -> Option<String> {
    path.split(['/', '\\'])
        .find(|part| part.len() > 4 && part[..4].eq_ignore_ascii_case("CAB-"))
        .map(|part| part.to_ascii_lowercase())
}

fn aabb_value(value: &UnityValue) -> Option<(Vec3, Vec3)> {
    let obj = value.as_object()?;
    let center = obj
        .get("m_Center")
        .or_else(|| obj.get("center"))
        .and_then(vec3_value)?;
    let extent = obj
        .get("m_Extent")
        .or_else(|| obj.get("m_Extents"))
        .or_else(|| obj.get("extent"))
        .or_else(|| obj.get("extents"))
        .and_then(vec3_value)?;
    Some((center, extent.abs()))
}

fn vec3_value(value: &UnityValue) -> Option<Vec3> {
    let obj = value.as_object()?;
    Some(Vec3::new(
        obj.get("x").and_then(f32_value)?,
        obj.get("y").and_then(f32_value)?,
        obj.get("z").and_then(f32_value)?,
    ))
}

fn quat_value(value: &UnityValue) -> Option<Quat> {
    let obj = value.as_object()?;
    Some(Quat::new(
        obj.get("x").and_then(f32_value)?,
        obj.get("y").and_then(f32_value)?,
        obj.get("z").and_then(f32_value)?,
        obj.get("w").and_then(f32_value)?,
    ))
}

fn f32_value(value: &UnityValue) -> Option<f32> {
    value.as_f64().map(|v| v as f32)
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    const ONE: Self = Self::new(1.0, 1.0, 1.0);

    const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    fn from_array(value: [f32; 3]) -> Self {
        Self::new(value[0], value[1], value[2])
    }

    fn to_array(self) -> [f32; 3] {
        [self.x, self.y, self.z]
    }

    fn add(self, other: Self) -> Self {
        Self::new(self.x + other.x, self.y + other.y, self.z + other.z)
    }

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(self.x * other.x, self.y * other.y, self.z * other.z)
    }

    fn mul_scalar(self, scalar: f32) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }

    fn abs(self) -> Self {
        Self::new(self.x.abs(), self.y.abs(), self.z.abs())
    }

    fn max_component(self) -> f32 {
        self.x.max(self.y).max(self.z)
    }

    fn magnitude(self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }

    fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y + self.z * self.z
    }

    fn lerp(self, other: Self, t: f32) -> Self {
        self.mul_scalar(1.0 - t).add(other.mul_scalar(t))
    }

    fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite() && self.z.is_finite()
    }
}

#[derive(Debug, Clone, Copy)]
struct Quat {
    x: f32,
    y: f32,
    z: f32,
    w: f32,
}

impl Quat {
    const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    const fn new(x: f32, y: f32, z: f32, w: f32) -> Self {
        Self { x, y, z, w }
    }

    fn from_euler_zxy(degrees: [f32; 3]) -> Self {
        let [x, y, z] = degrees.map(f32::to_radians);
        Self::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), y)
            .mul(Self::from_axis_angle(Vec3::new(1.0, 0.0, 0.0), x))
            .mul(Self::from_axis_angle(Vec3::new(0.0, 0.0, 1.0), z))
            .normalized()
    }

    fn from_array(value: [f32; 4]) -> Self {
        Self::new(value[0], value[1], value[2], value[3])
    }

    fn to_array(self) -> [f32; 4] {
        [self.x, self.y, self.z, self.w]
    }

    fn from_axis_angle(axis: Vec3, radians: f32) -> Self {
        let half = radians * 0.5;
        let (sin, cos) = half.sin_cos();
        Self::new(axis.x * sin, axis.y * sin, axis.z * sin, cos)
    }

    fn mul(self, other: Self) -> Self {
        Self::new(
            self.w * other.x + self.x * other.w + self.y * other.z - self.z * other.y,
            self.w * other.y - self.x * other.z + self.y * other.w + self.z * other.x,
            self.w * other.z + self.x * other.y - self.y * other.x + self.z * other.w,
            self.w * other.w - self.x * other.x - self.y * other.y - self.z * other.z,
        )
    }

    fn conjugate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    fn normalized(self) -> Self {
        let len = (self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w).sqrt();
        if len <= f32::EPSILON {
            Self::IDENTITY
        } else {
            Self::new(self.x / len, self.y / len, self.z / len, self.w / len)
        }
    }

    fn rotate(self, value: Vec3) -> Vec3 {
        let q = self.normalized();
        let v = Self::new(value.x, value.y, value.z, 0.0);
        let out = q.mul(v).mul(q.conjugate());
        Vec3::new(out.x, out.y, out.z)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_scope(kind: &str, id: &str, candidates: &[&str]) -> ScopeExtractionState {
        ScopeExtractionState {
            kind: kind.into(),
            id: id.into(),
            aliases: Vec::new(),
            candidates: candidates.iter().map(PathBuf::from).collect(),
            doc: CollisionGeometryDocument {
                version: GEOMETRY_VERSION.into(),
                scope_kind: kind.into(),
                scope_id: id.into(),
                coordinate_space: "local".into(),
                shapes: Vec::new(),
                warnings: Vec::new(),
            },
            source_bundle: None,
            source_hash: None,
            started: false,
            direct_candidates_done: 0,
            completed_direct_candidate_paths: HashSet::new(),
            done: false,
            completed: false,
        }
    }

    fn test_shape() -> CollisionShape {
        CollisionShape {
            id: "shape".into(),
            source_kind: SCOPE_ITEM.into(),
            source_id: "gate".into(),
            label: "shape".into(),
            provenance: Some(PROVENANCE_PREFAB_GRAPH.into()),
            source_asset: Some("gate".into()),
            object_path: Some("shape".into()),
            shape: "box_collider".into(),
            center: [0.0, 0.0, 0.0],
            half_extents: [1.0, 1.0, 1.0],
            rotation: Quat::IDENTITY.to_array(),
            confidence: 1.0,
        }
    }

    fn render_bounds_shape() -> CollisionShape {
        CollisionShape {
            shape: "mesh_bounds".into(),
            ..test_shape()
        }
    }

    fn helper_probe_shape() -> CollisionShape {
        CollisionShape {
            label: "LightProbes/Constraints/Box".into(),
            shape: "box_collider".into(),
            ..test_shape()
        }
    }

    fn collision_event(sample_index: usize, time: f64, pos: [f32; 3]) -> CollisionEvent {
        CollisionEvent {
            sample_index,
            capture_time_seconds: time,
            severity: 8,
            confidence: 0.8,
            speed_before: 8.0,
            speed_after: 0.0,
            speed_delta: 8.0,
            decel_mps2: 80.0,
            pos: Some(pos),
            geometry_confirmed: false,
            geometry_status: None,
            hit_source: None,
            hit_label: None,
            hit_shape: None,
            hit_distance: None,
        }
    }

    fn raw_candidate(file_key: usize, label: &str, hierarchy_path: &str) -> RawShapeCandidate {
        RawShapeCandidate {
            file_key,
            label: label.into(),
            hierarchy_path: hierarchy_path.into(),
            shape_name: "box_collider".into(),
            center: Vec3::ZERO,
            half_extents: Vec3::ONE,
            rotation: Quat::IDENTITY,
            confidence: 0.95,
        }
    }

    fn parsed_file_with_mesh_dependencies(
        archive_id: Option<&str>,
        external_archives: Vec<Option<&str>>,
        colliders: Vec<ColliderInfo>,
        meshes: Vec<(i64, MeshInfo)>,
    ) -> ParsedFileGeometry {
        ParsedFileGeometry {
            archive_id: archive_id.map(str::to_string),
            external_archives: external_archives
                .into_iter()
                .map(|archive| archive.map(str::to_string))
                .collect(),
            markers: HashSet::new(),
            gameobjects: HashMap::new(),
            transforms: HashMap::new(),
            meshes: meshes.into_iter().collect(),
            colliders,
        }
    }

    fn decoded_catalog_with_dependency() -> DecodedAddressablesCatalog {
        let keys = vec![
            Some("Assets/Scenes/Environments/BasketballCourt/BasketballCourt.unity".into()),
            Some("DependencyKey".into()),
        ];
        let (normalized_keys, key_indices_by_normalized_value) = normalized_catalog_keys(&keys);

        DecodedAddressablesCatalog {
            keys,
            normalized_keys,
            key_indices_by_normalized_value,
            buckets: vec![vec![0], vec![1]],
            entries: vec![[0, 0, 1, 0, 0, 0, 0], [1, 0, -1, 0, 0, 0, 0]],
            internal_ids: vec![
                "scene-location-without-bundle-extension".into(),
                "/aa/StandaloneOSX/dependency.bundle".into(),
            ],
        }
    }

    fn assert_progress_monotonic(events: &[GeometryExtractionProgress]) {
        for pair in events.windows(2) {
            assert!(
                pair[1].scopes_done >= pair[0].scopes_done,
                "scopes regressed from {} to {}",
                pair[0].scopes_done,
                pair[1].scopes_done
            );
            assert!(
                pair[1].bundles_done >= pair[0].bundles_done,
                "bundles regressed from {} to {}",
                pair[0].bundles_done,
                pair[1].bundles_done
            );
            assert!(
                pair[1].bundles_total >= pair[0].bundles_total,
                "bundle total regressed from {} to {}",
                pair[0].bundles_total,
                pair[1].bundles_total
            );
        }
    }

    #[test]
    fn geometry_progress_counts_unique_direct_and_reserved_shared_bundle_work() {
        let states = vec![
            test_scope(SCOPE_ITEM, "gate-a", &["one.bundle", "two.bundle"]),
            test_scope(SCOPE_ITEM, "gate-b", &["one.bundle", "two.bundle"]),
            test_scope(SCOPE_ENVIRONMENT, "env", &["env.bundle"]),
        ];

        let tracker = GeometryProgressTracker::new(&states);

        assert_eq!(tracker.scopes_total, 3);
        assert_eq!(tracker.bundles_total, 5);
    }

    #[test]
    fn environment_catalog_candidates_follow_scene_dependency_bundles() {
        let catalog = decoded_catalog_with_dependency();

        assert_eq!(
            catalog.candidate_bundle_names("BasketballCourt", true),
            vec!["dependency.bundle".to_string()]
        );
        assert_eq!(
            catalog.candidate_bundle_names("BasketballCourt", false),
            vec!["dependency.bundle".to_string()]
        );
    }

    #[test]
    fn mesh_dependency_archives_are_requested_only_when_missing() {
        let mesh_ref = PPtrRef {
            file_id: 1,
            path_id: 42,
        };
        let source = ParsedBundleGeometry {
            files: vec![parsed_file_with_mesh_dependencies(
                Some("cab-scene"),
                vec![Some("cab-mesh")],
                vec![ColliderInfo {
                    go_id: 1,
                    kind: ColliderKind::Mesh {
                        mesh_ref: Some(mesh_ref),
                    },
                }],
                Vec::new(),
            )],
        };
        let dependency = ParsedBundleGeometry {
            files: vec![parsed_file_with_mesh_dependencies(
                Some("cab-mesh"),
                Vec::new(),
                Vec::new(),
                vec![(
                    42,
                    MeshInfo {
                        center: Vec3::ZERO,
                        half_extents: Vec3::ONE,
                    },
                )],
            )],
        };

        assert_eq!(
            unresolved_mesh_dependency_archive_ids([&source]),
            BTreeSet::from(["cab-mesh".to_string()])
        );
        assert!(unresolved_mesh_dependency_archive_ids([&source, &dependency]).is_empty());
    }

    #[test]
    fn archive_name_matching_accepts_nodes_and_resource_siblings() {
        assert!(archive_name_matches(
            "CAB-76d2829f51375baf481bd3eb06055239",
            "cab-76d2829f51375baf481bd3eb06055239"
        ));
        assert!(archive_name_matches(
            "CAB-76d2829f51375baf481bd3eb06055239.resS",
            "cab-76d2829f51375baf481bd3eb06055239"
        ));
        assert!(!archive_name_matches(
            "CAB-other",
            "cab-76d2829f51375baf481bd3eb06055239"
        ));
    }

    #[test]
    fn environment_scope_rejects_unmarked_dependency_colliders() {
        let mut scene_markers = HashSet::new();
        scene_markers.insert("basketballcourtpaintedsteelgeneric01".into());
        let mut dependency_markers = HashSet::new();
        dependency_markers.insert("staircasekowloon01".into());
        let group = BundleGroupShapeCandidates {
            candidates: vec![
                raw_candidate(
                    0,
                    "Collider",
                    "MainStructure/Walls_Main/CourtSideWall01/Collider",
                ),
                raw_candidate(1, "UnrelatedCollider", "SharedPrefab/UnrelatedCollider"),
                raw_candidate(1, "StaircaseKowloon01", "StaircaseKowloon01"),
            ],
            file_markers: vec![scene_markers, dependency_markers],
            file_bundle_keys: vec![0, 1],
            warnings: Vec::new(),
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: SCOPE_ENVIRONMENT.into(),
            scope_id: "BasketballCourt".into(),
            coordinate_space: "world".into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };

        append_scope_shape_candidates(&group, SCOPE_ENVIRONMENT, "BasketballCourt", &[], &mut doc);

        assert_eq!(doc.shapes.len(), 1);
        assert_eq!(
            doc.shapes[0].label,
            "MainStructure/Walls_Main/CourtSideWall01/Collider"
        );
        assert_eq!(
            doc.shapes[0].provenance.as_deref(),
            Some(PROVENANCE_SCENE_GRAPH)
        );
        assert_eq!(
            doc.shapes[0].source_asset.as_deref(),
            Some("BasketballCourt")
        );
    }

    #[test]
    fn environment_scope_accepts_scene_colliders_when_sibling_file_marks_bundle() {
        let mut scene_markers = HashSet::new();
        scene_markers.insert("basketballcourtsettings".into());
        let group = BundleGroupShapeCandidates {
            candidates: vec![raw_candidate(
                1,
                "Collider",
                "MainStructure/Walls_Main/CourtSideWall01/Collider",
            )],
            file_markers: vec![scene_markers, HashSet::new()],
            file_bundle_keys: vec![0, 0],
            warnings: Vec::new(),
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: SCOPE_ENVIRONMENT.into(),
            scope_id: "BasketballCourt".into(),
            coordinate_space: "world".into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };

        append_scope_shape_candidates(&group, SCOPE_ENVIRONMENT, "BasketballCourt", &[], &mut doc);

        assert_eq!(doc.shapes.len(), 1);
        assert_eq!(
            doc.shapes[0].label,
            "MainStructure/Walls_Main/CourtSideWall01/Collider"
        );
    }

    #[test]
    fn environment_scope_does_not_accept_collider_label_without_scene_graph_marker() {
        let mut unrelated_markers = HashSet::new();
        unrelated_markers.insert("unrelatedscene".into());
        let group = BundleGroupShapeCandidates {
            candidates: vec![raw_candidate(
                0,
                "BasketballCourtCollider",
                "BasketballCourt/Wall/BasketballCourtCollider",
            )],
            file_markers: vec![unrelated_markers],
            file_bundle_keys: vec![0],
            warnings: Vec::new(),
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: SCOPE_ENVIRONMENT.into(),
            scope_id: "BasketballCourt".into(),
            coordinate_space: "world".into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };

        append_scope_shape_candidates(&group, SCOPE_ENVIRONMENT, "BasketballCourt", &[], &mut doc);

        assert!(doc.shapes.is_empty());
    }

    #[test]
    fn item_scope_requires_exact_prefab_marker_or_alias() {
        let mut broad_markers = HashSet::new();
        broad_markers.insert("airflagv01startfinish01baked".into());
        let mut alias_markers = HashSet::new();
        alias_markers.insert("airflagv01startfinish01".into());
        let group = BundleGroupShapeCandidates {
            candidates: vec![
                raw_candidate(0, "Collider", "AirFlagv01StartFinish01Baked/Collider"),
                raw_candidate(1, "Collider", "AirFlagv01StartFinish01/Collider"),
            ],
            file_markers: vec![broad_markers, alias_markers],
            file_bundle_keys: vec![0, 1],
            warnings: Vec::new(),
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: SCOPE_ITEM.into(),
            scope_id: "AirflagLiftoffFinish01".into(),
            coordinate_space: "local".into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };

        append_scope_shape_candidates(
            &group,
            SCOPE_ITEM,
            "AirflagLiftoffFinish01",
            &["Airflagv01StartFinish01".into()],
            &mut doc,
        );

        assert_eq!(doc.shapes.len(), 1);
        assert_eq!(doc.shapes[0].label, "AirFlagv01StartFinish01/Collider");
        assert_eq!(
            doc.shapes[0].provenance.as_deref(),
            Some(PROVENANCE_PREFAB_GRAPH)
        );
    }

    #[test]
    fn item_scope_filters_multi_prefab_bundle_to_matching_root() {
        let mut markers = HashSet::new();
        markers.insert("airgatebigliftofffinishwhite01".into());
        markers.insert("airgatebigliftoffwhite01".into());
        markers.insert("trussgatebox02".into());
        let group = BundleGroupShapeCandidates {
            candidates: vec![
                raw_candidate(
                    0,
                    "Collision",
                    "AirgateBigLiftoffWhite01/Collision/Collision",
                ),
                raw_candidate(
                    0,
                    "Collision",
                    "AirgateBigLiftoffFinishWhite01/Collision/Collision",
                ),
                raw_candidate(
                    0,
                    "Collider",
                    "TrussGateBox02/LOD_0/TrussBoxGateFrame01/Collider",
                ),
                raw_candidate(
                    0,
                    "Collision",
                    "AirgateBigLiftoffFinishWhite01/Collision/Collision",
                ),
            ],
            file_markers: vec![markers],
            file_bundle_keys: vec![0],
            warnings: Vec::new(),
        };
        let mut doc = CollisionGeometryDocument {
            version: GEOMETRY_VERSION.into(),
            scope_kind: SCOPE_ITEM.into(),
            scope_id: "AirgateBigLiftoffFinishWhite01".into(),
            coordinate_space: "local".into(),
            shapes: Vec::new(),
            warnings: Vec::new(),
        };

        append_scope_shape_candidates(
            &group,
            SCOPE_ITEM,
            "AirgateBigLiftoffFinishWhite01",
            &[],
            &mut doc,
        );

        assert_eq!(doc.shapes.len(), 2);
        assert!(doc
            .shapes
            .iter()
            .all(|shape| shape.label.starts_with("AirgateBigLiftoffFinishWhite01/")));
    }

    #[test]
    fn pending_bundle_work_includes_later_candidate_positions() {
        let mut states = vec![
            test_scope(SCOPE_ITEM, "gate-a", &["one.bundle", "two.bundle"]),
            test_scope(SCOPE_ITEM, "gate-b", &["zero.bundle", "one.bundle"]),
            test_scope(SCOPE_ENVIRONMENT, "env", &["one.bundle"]),
        ];
        mark_direct_candidate_processed(&mut states[1], Path::new("zero.bundle"));
        let scope_indices_by_bundle = direct_scope_indices_by_bundle(&states);

        let pending = pending_scope_indices_for_bundle(
            &states,
            &scope_indices_by_bundle,
            Path::new("one.bundle"),
        );

        assert_eq!(pending, vec![0, 1, 2]);
    }

    #[test]
    fn direct_bundle_progress_is_counted_once_per_path() {
        let states = vec![
            test_scope(SCOPE_ITEM, "gate-a", &["one.bundle"]),
            test_scope(SCOPE_ITEM, "gate-b", &["one.bundle"]),
        ];
        let mut tracker = GeometryProgressTracker::new(&states);

        tracker.complete_direct_bundle(Path::new("one.bundle"));
        tracker.complete_direct_bundle(Path::new("one.bundle"));

        assert_eq!(tracker.bundles_done, 1);
    }

    #[test]
    fn geometry_progress_skips_remaining_candidates_when_scope_completes() {
        let mut states = vec![test_scope(
            SCOPE_ITEM,
            "gate",
            &["one.bundle", "two.bundle", "three.bundle"],
        )];
        let mut tracker = GeometryProgressTracker::new(&states);
        let mut events = Vec::new();

        mark_direct_candidate_processed(&mut states[0], Path::new("one.bundle"));
        tracker.complete_direct_bundle(Path::new("one.bundle"));
        states[0].doc.shapes.push(test_shape());
        complete_scope(&mut states, 0, &mut tracker, &mut |event| {
            events.push(event);
        });

        assert_eq!(tracker.scopes_done, 1);
        assert_eq!(tracker.bundles_done, 3);
        assert_eq!(events.last().unwrap().bundles_done, 3);

        tracker.skip_inactive_shared_groups(&HashSet::new());
        assert_eq!(tracker.bundles_done, tracker.bundles_total);
    }

    #[test]
    fn geometry_progress_events_are_monotonic_through_completion() {
        let mut states = vec![test_scope(
            SCOPE_ITEM,
            "gate",
            &["one.bundle", "two.bundle", "three.bundle"],
        )];
        let mut tracker = GeometryProgressTracker::new(&states);
        let mut events = Vec::new();

        tracker.emit(
            &mut |event| events.push(event),
            "geometry_started",
            &states,
            None,
            None,
            None,
        );
        mark_direct_candidate_processed(&mut states[0], Path::new("one.bundle"));
        tracker.complete_direct_bundle(Path::new("one.bundle"));
        tracker.emit(
            &mut |event| events.push(event),
            "geometry_bundle_completed",
            &states,
            None,
            None,
            Some("one.bundle".into()),
        );
        states[0].doc.shapes.push(test_shape());
        complete_scope(&mut states, 0, &mut tracker, &mut |event| {
            events.push(event);
        });
        tracker.skip_inactive_shared_groups(&HashSet::new());
        tracker.emit(
            &mut |event| events.push(event),
            "geometry_completed",
            &states,
            None,
            None,
            None,
        );

        assert_progress_monotonic(&events);
        let last = events.last().unwrap();
        assert_eq!(last.scopes_done, last.scopes_total);
        assert_eq!(last.bundles_done, last.bundles_total);
    }

    #[test]
    fn distance_to_aabb_is_zero_inside_box() {
        let d = distance_point_aabb(
            Vec3::new(0.2, 0.0, 0.0),
            Vec3::ZERO,
            Vec3::new(1.0, 1.0, 1.0),
        );
        assert_eq!(d, 0.0);
    }

    #[test]
    fn distance_to_obb_uses_shape_rotation() {
        let rotation = Quat::from_axis_angle(Vec3::new(0.0, 1.0, 0.0), 45_f32.to_radians());
        let inside_rotated_box = rotation.rotate(Vec3::new(0.9, 0.0, 0.0));

        assert!(
            distance_point_aabb(inside_rotated_box, Vec3::ZERO, Vec3::new(1.0, 0.2, 0.2),) > 0.0
        );
        assert_eq!(
            distance_point_obb(
                inside_rotated_box,
                Vec3::ZERO,
                Vec3::new(1.0, 0.2, 0.2),
                rotation,
            ),
            0.0,
        );
    }

    #[test]
    fn missing_geometry_filters_out_all_telemetry_candidates() {
        let events = vec![
            collision_event(0, 0.0, [0.0, 0.0, 0.0]),
            collision_event(1, 1.0, [0.0, 0.0, 0.0]),
        ];

        let filtered = confirm_collision_events(&events, &[], None);
        assert!(filtered.is_empty());
    }

    #[test]
    fn geometry_confirmation_filters_out_non_matching_candidates() {
        let events = vec![
            collision_event(0, 0.0, [0.0, 0.0, 0.0]),
            collision_event(1, 1.0, [5.0, 0.0, 0.0]),
        ];
        let geometry = CourseCollisionGeometry {
            shapes: vec![test_shape()],
            warnings: Vec::new(),
            unavailable: false,
        };

        let filtered = confirm_collision_events(&events, &[], Some(&geometry));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].sample_index, 0);
        assert!(filtered[0].geometry_confirmed);
        assert_eq!(filtered[0].geometry_status.as_deref(), Some("confirmed"));
    }

    #[test]
    fn render_mesh_bounds_do_not_confirm_collision_candidates() {
        let events = vec![collision_event(0, 0.0, [0.0, 0.0, 0.0])];
        let geometry = CourseCollisionGeometry {
            shapes: vec![render_bounds_shape()],
            warnings: Vec::new(),
            unavailable: false,
        };

        let filtered = confirm_collision_events(&events, &[], Some(&geometry));

        assert!(filtered.is_empty());
    }

    #[test]
    fn helper_probe_box_colliders_do_not_confirm_collision_candidates() {
        let events = vec![collision_event(0, 0.0, [0.0, 0.0, 0.0])];
        let geometry = CourseCollisionGeometry {
            shapes: vec![helper_probe_shape()],
            warnings: Vec::new(),
            unavailable: false,
        };

        let filtered = confirm_collision_events(&events, &[], Some(&geometry));

        assert!(filtered.is_empty());
    }

    #[test]
    fn partial_geometry_uses_available_shapes_without_fallback_candidates() {
        let events = vec![
            collision_event(0, 0.0, [0.0, 0.0, 0.0]),
            collision_event(1, 1.0, [5.0, 0.0, 0.0]),
        ];
        let geometry = CourseCollisionGeometry {
            shapes: vec![test_shape()],
            warnings: vec!["some geometry unavailable".into()],
            unavailable: true,
        };

        let filtered = confirm_collision_events(&events, &[], Some(&geometry));

        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].sample_index, 0);
        assert!(filtered[0].geometry_confirmed);
    }

    #[test]
    fn procedural_ribbon_shapes_are_marked_as_track_xml_procedural() {
        let prop = ReplayCourseProp {
            instance_id: 42,
            item_id: "Ribbon01".into(),
            kind: "collision_prop".into(),
            position: [0.0, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            dimensions: None,
            attach_points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
            procedural_geometry: true,
            name: None,
        };

        let shapes = procedural_ribbon_shapes(&prop);

        assert_eq!(shapes.len(), 1);
        assert_eq!(
            shapes[0].provenance.as_deref(),
            Some(PROVENANCE_TRACK_XML_PROCEDURAL)
        );
        assert_eq!(shapes[0].source_asset.as_deref(), Some("Ribbon01"));
    }

    #[test]
    fn environment_key_hits_prefer_scene_assets() {
        let keys = vec![
            Some("AutumnFields".to_string()),
            Some("Assets/Scenes/Environments/AutumnFields/AutumnFields.unity".to_string()),
            Some(
                "Assets/Scenes/Environments/AutumnFields/AutumnFields_Profiles/PostProcessing.asset"
                    .to_string(),
            ),
        ];
        let (normalized_keys, _) = normalized_catalog_keys(&keys);

        let hits = environment_key_hits(&keys, &normalized_keys, "AutumnFields");

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].index, 1);
    }

    #[test]
    fn environment_key_hits_use_exact_binary_keys_when_scene_assets_are_absent() {
        let keys = vec![
            Some(
                "Assets/Scenes/Environments/JapanesePlayground/JapanesePlayground_Profiles/PostProcessing.asset"
                    .to_string(),
            ),
            Some("JapanesePlayground".to_string()),
            Some("JapanesePlayground_Empty".to_string()),
            Some("Assets/Project/Textures/UI/LevelBackgrounds/JapanesePlayground.png".to_string()),
        ];
        let (normalized_keys, _) = normalized_catalog_keys(&keys);

        let hits = environment_key_hits(&keys, &normalized_keys, "JapanesePlayground");
        let hit_indices = hits.iter().map(|hit| hit.index).collect::<Vec<_>>();

        assert_eq!(hit_indices, vec![1]);
    }

    #[test]
    fn environment_key_hits_prefer_default_environment_key_over_empty_variant() {
        let keys = vec![
            Some("BasketBallCourt_Empty".to_string()),
            Some("BasketBallCourt".to_string()),
            Some("BasketballCourtItems".to_string()),
        ];
        let (normalized_keys, _) = normalized_catalog_keys(&keys);

        let hits = environment_key_hits(&keys, &normalized_keys, "BasketballCourt");
        let hit_indices = hits.iter().map(|hit| hit.index).collect::<Vec<_>>();

        assert_eq!(hit_indices, vec![1]);
    }

    #[test]
    fn environment_key_hits_use_day_variant_before_empty_fallback() {
        let keys = vec![
            Some("SomeEnvironment_Empty".to_string()),
            Some("SomeEnvironmentDay".to_string()),
            Some("SomeEnvironmentBackdrop".to_string()),
        ];
        let (normalized_keys, _) = normalized_catalog_keys(&keys);

        let hits = environment_key_hits(&keys, &normalized_keys, "SomeEnvironment");
        let hit_indices = hits.iter().map(|hit| hit.index).collect::<Vec<_>>();

        assert_eq!(hit_indices, vec![1]);
    }
}
