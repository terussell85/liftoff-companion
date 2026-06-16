use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use chrono::Utc;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use unity_asset::environment::{Environment, EnvironmentObjectRef};
use unity_asset::UnityValue;

use crate::capture::integrity::hash_bytes;
use crate::error::{AppError, AppResult};
use crate::liftoff::geometry::{self, GeometryExtractionProgress, GeometryScope};
use crate::storage::repositories::{
    self, CollisionGeometryCacheRow, GameAssetCacheRow, RaceCourseCacheRow, RaceSessionRow,
};

pub const EXTRACTOR_VERSION: &str = "liftoff-assets-v13";

const XML_START: &[u8] = b"<?xml";
const TRACK_END: &str = "</Track>";
const RACE_END: &str = "</Race>";
const MAX_XML_BUNDLE_SCAN_BYTES: u64 = 5 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GameAssetSourceStatus {
    pub game_title: String,
    pub label: String,
    pub data_root: String,
    pub valid: bool,
    pub cache_status: String,
    pub cache_id: Option<String>,
    pub extracted_at: Option<chrono::DateTime<Utc>>,
    pub race_count: i64,
    pub track_count: i64,
    pub error_message: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameAssetCatalog {
    pub cache_id: String,
    pub game_title: String,
    pub data_root: String,
    pub levels: Vec<GameAssetLevelCatalog>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameAssetLevelCatalog {
    pub environment_id: Option<String>,
    pub name: String,
    pub races: Vec<GameAssetRaceCatalog>,
}

#[derive(Debug, Clone, Serialize)]
pub struct GameAssetRaceCatalog {
    pub race_guid: String,
    pub race_name: String,
    pub race_asset_key: Option<String>,
    pub track_guid: Option<String>,
    pub track_name: Option<String>,
    pub track_asset_key: Option<String>,
    pub required_laps: Option<i64>,
    pub checkpoint_count: usize,
    pub prop_count: usize,
    pub collision_prop_count: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RefreshRaceTrackCacheRequest {
    pub force: Option<bool>,
    pub data_root: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AssetRefreshProgress {
    pub phase: String,
    pub message: String,
    pub game_title: Option<String>,
    pub data_root: Option<String>,
    pub sources_done: u64,
    pub sources_total: u64,
    pub scopes_done: u64,
    pub scopes_total: u64,
    pub levels_done: u64,
    pub levels_total: u64,
    pub bundles_done: u64,
    pub bundles_total: u64,
    pub current_scope: Option<String>,
    pub current_level: Option<String>,
    pub current_bundle: Option<String>,
    pub races_found: u64,
    pub tracks_found: u64,
    pub geometry_ready: u64,
    pub geometry_partial: u64,
    pub geometry_missing: u64,
    pub geometry_shapes: u64,
}

impl AssetRefreshProgress {
    pub fn new(phase: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            phase: phase.into(),
            message: message.into(),
            game_title: None,
            data_root: None,
            sources_done: 0,
            sources_total: 0,
            scopes_done: 0,
            scopes_total: 0,
            levels_done: 0,
            levels_total: 0,
            bundles_done: 0,
            bundles_total: 0,
            current_scope: None,
            current_level: None,
            current_bundle: None,
            races_found: 0,
            tracks_found: 0,
            geometry_ready: 0,
            geometry_partial: 0,
            geometry_missing: 0,
            geometry_shapes: 0,
        }
    }

    fn with_source(mut self, source: &AssetSource, sources_done: u64, sources_total: u64) -> Self {
        self.game_title = Some(source.game_title.clone());
        self.data_root = Some(path_string(&source.data_root));
        self.sources_done = sources_done;
        self.sources_total = sources_total;
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSessionCourseRequest {
    pub capture_id: String,
    pub session_id: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResolveSessionCourseResponse {
    pub course: Option<ReplayCourseData>,
    pub refreshed: bool,
    pub status: String,
    pub message: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCourseData {
    pub cache_id: String,
    pub game_title: String,
    pub data_root: String,
    pub race_guid: String,
    pub race_name: String,
    #[serde(default)]
    pub race_asset_key: Option<String>,
    pub track_guid: Option<String>,
    pub track_name: Option<String>,
    #[serde(default)]
    pub track_asset_key: Option<String>,
    pub environment_id: Option<String>,
    pub required_laps: Option<i64>,
    pub checkpoints: Vec<ReplayCheckpoint>,
    pub spawnpoint: Option<ReplayCourseProp>,
    pub props: Vec<ReplayCourseProp>,
    #[serde(default)]
    pub collision_props: Vec<ReplayCourseProp>,
    #[serde(default)]
    pub guide_path: Option<ReplayGuidePath>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCheckpoint {
    pub sequence_index: i64,
    pub checkpoint_id: i64,
    pub passage_type: String,
    pub directionality: String,
    pub item_id: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub dimensions: [f32; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayCourseProp {
    pub instance_id: i64,
    pub item_id: String,
    pub kind: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub dimensions: Option<[f32; 3]>,
    pub attach_points: Vec<[f32; 3]>,
    pub procedural_geometry: bool,
    pub name: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGuidePath {
    pub algorithm: String,
    pub accuracy: String,
    pub segments: Vec<ReplayGuidePathSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReplayGuidePathSegment {
    pub from_passage_id: Option<String>,
    pub to_passage_id: Option<String>,
    pub from_checkpoint_id: Option<i64>,
    pub to_checkpoint_id: Option<i64>,
    pub points: Vec<[f32; 3]>,
}

#[derive(Debug, Clone)]
pub struct AssetSource {
    pub game_title: String,
    pub label: String,
    pub data_root: PathBuf,
}

#[derive(Debug, Clone, Serialize)]
struct SourceFingerprint {
    extractor_version: String,
    data_root: String,
    files: Vec<SourceFileFingerprint>,
}

#[derive(Debug, Clone, Serialize)]
struct SourceFileFingerprint {
    relative_path: String,
    size_bytes: u64,
    modified_ms: Option<i64>,
}

#[derive(Debug)]
struct ExtractedInstall {
    cache: GameAssetCacheRow,
    courses: Vec<RaceCourseCacheRow>,
    geometry: Vec<CollisionGeometryCacheRow>,
    geometry_progress: Option<GeometryExtractionProgress>,
}

#[derive(Debug, Clone)]
struct TrackXml {
    guid: String,
    name: String,
    asset_key: Option<String>,
    environment_id: Option<String>,
    blueprints: Vec<TrackBlueprint>,
}

#[derive(Debug, Clone)]
struct RaceXml {
    guid: String,
    name: String,
    asset_key: Option<String>,
    track_guid: Option<String>,
    required_laps: Option<i64>,
    spawnpoint_id: Option<i64>,
    passages: Vec<RacePassage>,
}

#[derive(Debug, Clone)]
struct RacePassage {
    unique_id: String,
    checkpoint_id: i64,
    passage_type: String,
    directionality: String,
    next_ids: Vec<String>,
}

#[derive(Debug, Clone)]
struct TrackBlueprint {
    kind: String,
    item_id: String,
    instance_id: i64,
    position: Option<[f32; 3]>,
    rotation: Option<[f32; 3]>,
    dimensions: Option<[f32; 3]>,
    attach_points: Vec<[f32; 3]>,
    name: Option<String>,
}

pub fn list_sources_with_cache(conn: &Connection) -> AppResult<Vec<GameAssetSourceStatus>> {
    let cache_rows = repositories::list_game_asset_caches(conn)?;
    let by_root: HashMap<String, GameAssetCacheRow> = cache_rows
        .into_iter()
        .map(|row| (row.data_root.clone(), row))
        .collect();

    let mut out = Vec::new();
    let mut seen_roots = HashSet::new();
    for source in discover_asset_sources() {
        let root = path_string(&source.data_root);
        seen_roots.insert(root.clone());
        let row = by_root.get(&root);
        out.push(source_status(&source, row)?);
    }
    for (root, row) in &by_root {
        if seen_roots.contains(root) {
            continue;
        }
        let data_root = PathBuf::from(root);
        if is_unity_data_root(&data_root) {
            let source = AssetSource {
                game_title: row.game_title.clone(),
                label: "Cached path".into(),
                data_root,
            };
            out.push(source_status(&source, Some(row))?);
        } else {
            out.push(GameAssetSourceStatus {
                game_title: row.game_title.clone(),
                label: "Cached path".into(),
                data_root: root.clone(),
                valid: false,
                cache_status: "stale".into(),
                cache_id: Some(row.id.clone()),
                extracted_at: row.extracted_at,
                race_count: row.race_count,
                track_count: row.track_count,
                error_message: Some("Cached game data path no longer exists.".into()),
            });
        }
    }
    Ok(out)
}

pub fn list_asset_catalog(conn: &Connection) -> AppResult<Vec<GameAssetCatalog>> {
    let rows = repositories::list_race_course_caches(conn)?;
    let mut installs = BTreeMap::<String, GameAssetCatalog>::new();
    let mut levels = BTreeMap::<(String, String), GameAssetLevelCatalog>::new();

    for (cache, row) in rows {
        let course = serde_json::from_str::<ReplayCourseData>(&row.course_json).ok();
        let environment_id = course
            .as_ref()
            .and_then(|course| course.environment_id.clone())
            .or_else(|| row.environment_id.clone());
        let level_key = environment_id.clone().unwrap_or_else(|| "unknown".into());
        let level_name = environment_id
            .clone()
            .unwrap_or_else(|| "Unknown Level".into());
        let race = GameAssetRaceCatalog {
            race_guid: row.race_guid,
            race_name: row.race_name,
            race_asset_key: course
                .as_ref()
                .and_then(|course| course.race_asset_key.clone()),
            track_guid: row.track_guid,
            track_name: row.track_name,
            track_asset_key: course
                .as_ref()
                .and_then(|course| course.track_asset_key.clone()),
            required_laps: row.required_laps,
            checkpoint_count: course
                .as_ref()
                .map(|course| course.checkpoints.len())
                .unwrap_or_default(),
            prop_count: course
                .as_ref()
                .map(|course| course.props.len())
                .unwrap_or_default(),
            collision_prop_count: course
                .as_ref()
                .map(|course| course.collision_props.len())
                .unwrap_or_default(),
        };

        installs
            .entry(cache.id.clone())
            .or_insert_with(|| GameAssetCatalog {
                cache_id: cache.id.clone(),
                game_title: cache.game_title.clone(),
                data_root: cache.data_root.clone(),
                levels: Vec::new(),
            });

        levels
            .entry((cache.id, level_key))
            .or_insert_with(|| GameAssetLevelCatalog {
                environment_id,
                name: level_name,
                races: Vec::new(),
            })
            .races
            .push(race);
    }

    for ((cache_id, _), mut level) in levels {
        level.races.sort_by(|a, b| {
            a.race_name
                .cmp(&b.race_name)
                .then(a.race_guid.cmp(&b.race_guid))
        });
        if let Some(install) = installs.get_mut(&cache_id) {
            install.levels.push(level);
        }
    }

    let mut out = installs.into_values().collect::<Vec<_>>();
    for install in &mut out {
        install.levels.sort_by(|a, b| a.name.cmp(&b.name));
    }
    Ok(out)
}

pub fn refresh_sources(
    conn: &mut Connection,
    force: bool,
    manual_data_root: Option<&str>,
    mut progress: impl FnMut(AssetRefreshProgress),
) -> AppResult<Vec<GameAssetSourceStatus>> {
    progress(AssetRefreshProgress::new(
        "discovering_sources",
        "Finding Liftoff game data sources.",
    ));
    let sources = match manual_data_root {
        Some(path) => vec![manual_source(path)?],
        None => refreshable_sources(conn)?,
    };
    let sources_total = sources.len() as u64;
    progress({
        let mut event = AssetRefreshProgress::new(
            "sources_discovered",
            format!("Found {sources_total} refreshable game data source(s)."),
        );
        event.sources_total = sources_total;
        event
    });

    let mut statuses = Vec::new();
    let mut last_geometry_progress = None;
    for (source_index, source) in sources.into_iter().enumerate() {
        let sources_done = source_index as u64;
        let root = path_string(&source.data_root);
        progress(
            AssetRefreshProgress::new(
                "source_started",
                format!("Checking {} game data.", source.game_title),
            )
            .with_source(&source, sources_done, sources_total),
        );
        let existing = repositories::get_game_asset_cache_by_root(conn, &root)?;
        let current_status = source_status(&source, existing.as_ref())?;
        if !force
            && (current_status.cache_status == "fresh" || current_status.cache_status == "error")
        {
            progress(
                AssetRefreshProgress::new(
                    "source_skipped",
                    format!(
                        "Using existing {} cache ({})",
                        source.game_title, current_status.cache_status
                    ),
                )
                .with_source(&source, sources_done + 1, sources_total),
            );
            statuses.push(current_status);
            continue;
        }

        match extract_install(&source, sources_done, sources_total, &mut progress) {
            Ok(extracted) => {
                let geometry_ready = extracted
                    .geometry
                    .iter()
                    .filter(|row| should_report_geometry_status(row) && row.status == "ready")
                    .count() as u64;
                let geometry_partial = extracted
                    .geometry
                    .iter()
                    .filter(|row| should_report_geometry_status(row) && row.status == "partial")
                    .count() as u64;
                let geometry_missing = extracted
                    .geometry
                    .iter()
                    .filter(|row| should_report_geometry_status(row) && row.status == "missing")
                    .count() as u64;
                let geometry_shapes = count_geometry_shapes(&extracted.geometry);
                last_geometry_progress = extracted.geometry_progress.clone();
                progress({
                    let mut event = AssetRefreshProgress::new(
                        "storing_cache",
                        format!(
                            "Saving {} races, {} tracks, and {} geometry scope(s).",
                            extracted.cache.race_count,
                            extracted.cache.track_count,
                            extracted.geometry.len()
                        ),
                    )
                    .with_source(&source, sources_done, sources_total);
                    event.races_found = extracted.cache.race_count.max(0) as u64;
                    event.tracks_found = extracted.cache.track_count.max(0) as u64;
                    event.geometry_ready = geometry_ready;
                    event.geometry_partial = geometry_partial;
                    event.geometry_missing = geometry_missing;
                    event.geometry_shapes = geometry_shapes;
                    apply_geometry_progress(
                        &mut event,
                        extracted.geometry_progress.as_ref(),
                        geometry_shapes,
                    );
                    event
                });
                repositories::replace_game_asset_cache(conn, &extracted.cache, &extracted.courses)?;
                repositories::replace_collision_geometry_cache(
                    conn,
                    &extracted.cache.id,
                    &extracted.geometry,
                )?;
                progress({
                    let mut event = AssetRefreshProgress::new(
                        "source_completed",
                        format!("Finished refreshing {}.", source.game_title),
                    )
                    .with_source(&source, sources_done + 1, sources_total);
                    event.races_found = extracted.cache.race_count.max(0) as u64;
                    event.tracks_found = extracted.cache.track_count.max(0) as u64;
                    event.geometry_ready = geometry_ready;
                    event.geometry_partial = geometry_partial;
                    event.geometry_missing = geometry_missing;
                    event.geometry_shapes = geometry_shapes;
                    apply_geometry_progress(
                        &mut event,
                        extracted.geometry_progress.as_ref(),
                        geometry_shapes,
                    );
                    event
                });
            }
            Err(error) => {
                progress(
                    AssetRefreshProgress::new(
                        "source_failed",
                        format!("{} refresh failed: {error}", source.game_title),
                    )
                    .with_source(&source, sources_done + 1, sources_total),
                );
                let (fingerprint_json, fingerprint_hash) = source_fingerprint(&source.data_root)
                    .unwrap_or_else(|_| {
                        let fallback = serde_json::json!({
                            "extractor_version": EXTRACTOR_VERSION,
                            "data_root": root,
                            "files": [],
                        })
                        .to_string();
                        let hash = hash_bytes(fallback.as_bytes());
                        (fallback, hash)
                    });
                let cache = GameAssetCacheRow {
                    id: cache_id_for_root(&source.data_root),
                    game_title: source.game_title.clone(),
                    data_root: root.clone(),
                    extractor_version: EXTRACTOR_VERSION.into(),
                    source_fingerprint_hash: fingerprint_hash,
                    source_fingerprint_json: fingerprint_json,
                    status: "error".into(),
                    error_message: Some(error.to_string()),
                    extracted_at: Some(Utc::now()),
                    race_count: 0,
                    track_count: 0,
                };
                repositories::replace_game_asset_cache(conn, &cache, &[])?;
            }
        }

        let refreshed = repositories::get_game_asset_cache_by_root(conn, &root)?;
        statuses.push(source_status(&source, refreshed.as_ref())?);
    }
    progress({
        let mut event =
            AssetRefreshProgress::new("refresh_completed", "Race/track data refresh complete.");
        event.sources_done = sources_total;
        event.sources_total = sources_total;
        apply_geometry_progress(
            &mut event,
            last_geometry_progress.as_ref(),
            last_geometry_progress
                .as_ref()
                .map(|progress| progress.shapes_found)
                .unwrap_or(0),
        );
        event
    });
    Ok(statuses)
}

fn refreshable_sources(conn: &Connection) -> AppResult<Vec<AssetSource>> {
    let mut sources = discover_asset_sources();
    let mut seen: HashSet<String> = sources
        .iter()
        .map(|source| path_string(&source.data_root))
        .collect();

    for row in repositories::list_game_asset_caches(conn)? {
        if seen.contains(&row.data_root) {
            continue;
        }
        let data_root = PathBuf::from(&row.data_root);
        if is_unity_data_root(&data_root) {
            seen.insert(row.data_root);
            sources.push(AssetSource {
                game_title: row.game_title,
                label: "Cached path".into(),
                data_root,
            });
        }
    }

    Ok(sources)
}

pub fn resolve_session_course(
    conn: &Connection,
    session: &RaceSessionRow,
) -> AppResult<Option<ReplayCourseData>> {
    let candidates = repositories::list_race_course_caches(conn)?;
    let mut best: Option<(i32, ReplayCourseData)> = None;

    for (cache, course_row) in candidates {
        if !cache_row_currently_fresh(&cache) {
            continue;
        }

        let score = course_match_score(session, &cache, &course_row);
        if score <= 0 {
            continue;
        }

        let course: ReplayCourseData = serde_json::from_str(&course_row.course_json)?;
        match &best {
            Some((best_score, _)) if *best_score >= score => {}
            _ => best = Some((score, course)),
        }
    }

    Ok(best.map(|(_, course)| course))
}

fn cache_row_currently_fresh(cache: &GameAssetCacheRow) -> bool {
    if cache.status != "fresh" || cache.extractor_version != EXTRACTOR_VERSION {
        return false;
    }
    let data_root = PathBuf::from(&cache.data_root);
    source_fingerprint(&data_root)
        .map(|(_, hash)| hash == cache.source_fingerprint_hash)
        .unwrap_or(false)
}

pub fn any_source_needs_refresh(conn: &Connection) -> AppResult<bool> {
    Ok(list_sources_with_cache(conn)?
        .iter()
        .any(|status| status.valid && matches!(status.cache_status.as_str(), "missing" | "stale")))
}

pub fn discover_asset_sources() -> Vec<AssetSource> {
    let mut sources = Vec::new();
    let mut seen = HashSet::new();

    for (title, label, path) in known_install_candidates() {
        push_valid_source(&mut sources, &mut seen, title, label, &path);
    }

    for library in steam_libraries() {
        let steamapps = library.join("steamapps");
        let manifests = match fs::read_dir(&steamapps) {
            Ok(entries) => entries,
            Err(_) => continue,
        };
        for entry in manifests.flatten() {
            let path = entry.path();
            let file_name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if !file_name.starts_with("appmanifest_") || !file_name.ends_with(".acf") {
                continue;
            }
            let text = match fs::read_to_string(&path) {
                Ok(text) => text,
                Err(_) => continue,
            };
            let name = extract_vdf_value(&text, "name").unwrap_or_default();
            if !is_liftoff_game_name(&name) {
                continue;
            }
            let installdir = extract_vdf_value(&text, "installdir").unwrap_or(name.clone());
            let install_root = steamapps.join("common").join(installdir);
            let title = game_title_from_name(&name, &install_root);
            push_valid_source(
                &mut sources,
                &mut seen,
                &title,
                format!("Steam · {}", name),
                &install_root,
            );
        }
    }

    sources
}

fn extract_install(
    source: &AssetSource,
    sources_done: u64,
    sources_total: u64,
    progress: &mut impl FnMut(AssetRefreshProgress),
) -> AppResult<ExtractedInstall> {
    progress(
        AssetRefreshProgress::new("fingerprinting", "Scanning game data file metadata.")
            .with_source(source, sources_done, sources_total),
    );
    let (fingerprint_json, fingerprint_hash) = source_fingerprint(&source.data_root)?;

    progress(
        AssetRefreshProgress::new(
            "xml_extracting",
            "Extracting race and track XML from Unity assets.",
        )
        .with_source(source, sources_done, sources_total),
    );
    let xml_items = extract_xml_items(&source.data_root)?;
    let mut tracks = HashMap::<String, TrackXml>::new();
    let mut races = Vec::<RaceXml>::new();
    let mut race_indices_by_guid = HashMap::<String, usize>::new();

    for item in xml_items {
        match item {
            XmlItem::Track(track) => {
                if !track.guid.is_empty() {
                    match tracks.get_mut(&track.guid) {
                        Some(existing) => {
                            if existing.asset_key.is_none() && track.asset_key.is_some() {
                                existing.asset_key = track.asset_key;
                            }
                        }
                        None => {
                            tracks.insert(track.guid.clone(), track);
                        }
                    }
                }
            }
            XmlItem::Race(race) => {
                if race.guid.is_empty() {
                    continue;
                }
                if let Some(index) = race_indices_by_guid.get(&race.guid).copied() {
                    if races[index].asset_key.is_none() && race.asset_key.is_some() {
                        races[index].asset_key = race.asset_key;
                    }
                } else {
                    race_indices_by_guid.insert(race.guid.clone(), races.len());
                    races.push(race);
                }
            }
        }
    }

    let cache_id = cache_id_for_root(&source.data_root);
    progress({
        let mut event = AssetRefreshProgress::new(
            "courses_building",
            format!(
                "Building replay courses from {} races and {} tracks.",
                races.len(),
                tracks.len()
            ),
        )
        .with_source(source, sources_done, sources_total);
        event.races_found = races.len() as u64;
        event.tracks_found = tracks.len() as u64;
        event
    });
    let mut courses = Vec::new();
    for race in &races {
        if let Some(course) = build_course(&cache_id, source, race, &tracks)? {
            courses.push(course);
        }
    }

    progress({
        let mut event =
            AssetRefreshProgress::new("geometry_preparing", "Preparing collision geometry scopes.")
                .with_source(source, sources_done, sources_total);
        event.races_found = races.len() as u64;
        event.tracks_found = tracks.len() as u64;
        event
    });
    let mut last_geometry_progress = None;
    let geometry = geometry::extract_collision_geometry_cache(
        &source.data_root,
        &cache_id,
        geometry_scopes_for_tracks_and_races(tracks.values(), &races),
        |geometry_progress| {
            last_geometry_progress = Some(geometry_progress.clone());
            let mut event = asset_progress_from_geometry(
                source,
                sources_done,
                sources_total,
                races.len() as u64,
                tracks.len() as u64,
                geometry_progress,
            );
            event.geometry_ready = 0;
            event.geometry_partial = 0;
            event.geometry_missing = 0;
            progress(event);
        },
    );

    let cache = GameAssetCacheRow {
        id: cache_id,
        game_title: source.game_title.clone(),
        data_root: path_string(&source.data_root),
        extractor_version: EXTRACTOR_VERSION.into(),
        source_fingerprint_hash: fingerprint_hash,
        source_fingerprint_json: fingerprint_json,
        status: "fresh".into(),
        error_message: None,
        extracted_at: Some(Utc::now()),
        race_count: races.len() as i64,
        track_count: tracks.len() as i64,
    };

    Ok(ExtractedInstall {
        cache,
        courses,
        geometry,
        geometry_progress: last_geometry_progress,
    })
}

fn asset_progress_from_geometry(
    source: &AssetSource,
    sources_done: u64,
    sources_total: u64,
    races_found: u64,
    tracks_found: u64,
    geometry: GeometryExtractionProgress,
) -> AssetRefreshProgress {
    let shapes_found = geometry.shapes_found;
    let current_scope = match (
        geometry.current_scope_kind.as_deref(),
        geometry.current_scope_id.as_deref(),
    ) {
        (Some(kind), Some(id)) => Some(format!("{kind}:{id}")),
        _ => None,
    };
    let current_level = geometry.current_level.clone();
    let message = match geometry.phase.as_str() {
        "geometry_started" => format!(
            "Extracting collision geometry for {} level scene(s).",
            geometry.levels_total
        ),
        "geometry_scope_started" => current_level
            .as_ref()
            .map(|level| format!("Scanning level scene {level}."))
            .unwrap_or_else(|| "Resolving track-object collision geometry.".into()),
        "geometry_bundle_started" => geometry
            .current_level
            .as_ref()
            .map(|level| format!("Reading scene data for {level}."))
            .unwrap_or_else(|| "Reading collision geometry data.".into()),
        "geometry_bundle_completed" => format!(
            "Scanned {}/{} level scene(s); {} collision shape(s) found.",
            geometry.levels_done, geometry.levels_total, geometry.shapes_found
        ),
        "geometry_item_group_started" => current_scope
            .as_ref()
            .map(|scope| format!("Resolving reusable track-object geometry for {scope}."))
            .unwrap_or_else(|| "Resolving reusable track-object geometry.".into()),
        "geometry_item_group_bundle_started" => "Reading reusable object geometry.".into(),
        "geometry_item_group_bundle_completed" => "Reusable object geometry read.".into(),
        "geometry_scope_completed" => current_level
            .as_ref()
            .map(|level| {
                format!(
                    "Finished level scene {level}; {}/{} level scene(s) scanned.",
                    geometry.levels_done, geometry.levels_total
                )
            })
            .unwrap_or_else(|| {
                format!(
                    "Resolved track-object geometry; {} shape(s) found.",
                    geometry.shapes_found
                )
            }),
        "geometry_completed" => format!(
            "Collision geometry extraction complete for {} level scene(s); {} shape(s) found.",
            geometry.levels_total, geometry.shapes_found
        ),
        _ => "Extracting collision geometry.".into(),
    };

    let mut event = AssetRefreshProgress::new(geometry.phase, message).with_source(
        source,
        sources_done,
        sources_total,
    );
    event.scopes_done = geometry.scopes_done;
    event.scopes_total = geometry.scopes_total;
    event.levels_done = geometry.levels_done;
    event.levels_total = geometry.levels_total;
    event.bundles_done = geometry.bundles_done;
    event.bundles_total = geometry.bundles_total;
    event.current_scope = current_scope;
    event.current_level = current_level;
    event.current_bundle = geometry.current_bundle;
    event.races_found = races_found;
    event.tracks_found = tracks_found;
    event.geometry_shapes = shapes_found;
    event
}

fn apply_geometry_progress(
    event: &mut AssetRefreshProgress,
    geometry: Option<&GeometryExtractionProgress>,
    shapes_found: u64,
) {
    let Some(geometry) = geometry else {
        return;
    };
    event.scopes_done = geometry.scopes_done;
    event.scopes_total = geometry.scopes_total;
    event.levels_done = geometry.levels_done;
    event.levels_total = geometry.levels_total;
    event.bundles_done = geometry.bundles_done;
    event.bundles_total = geometry.bundles_total;
    event.current_scope = match (
        geometry.current_scope_kind.as_deref(),
        geometry.current_scope_id.as_deref(),
    ) {
        (Some(kind), Some(id)) => Some(format!("{kind}:{id}")),
        _ => None,
    };
    event.current_level = geometry.current_level.clone();
    event.current_bundle = geometry.current_bundle.clone();
    event.geometry_shapes = shapes_found;
}

fn count_geometry_shapes(rows: &[CollisionGeometryCacheRow]) -> u64 {
    rows.iter()
        .filter_map(|row| {
            serde_json::from_str::<geometry::CollisionGeometryDocument>(&row.geometry_json).ok()
        })
        .map(|doc| doc.shapes.len() as u64)
        .sum()
}

fn should_report_geometry_status(row: &CollisionGeometryCacheRow) -> bool {
    row.scope_kind != "race" || row.status != "missing"
}

fn build_course(
    cache_id: &str,
    source: &AssetSource,
    race: &RaceXml,
    tracks: &HashMap<String, TrackXml>,
) -> AppResult<Option<RaceCourseCacheRow>> {
    let track = race
        .track_guid
        .as_ref()
        .and_then(|guid| tracks.get(guid))
        .or_else(|| {
            let race_name = normalize_key(&race.name);
            tracks
                .values()
                .find(|track| normalize_key(&track.name) == race_name)
        });
    let Some(track) = track else {
        return Ok(None);
    };

    let checkpoints_by_id = checkpoint_blueprints_by_id(&track.blueprints);
    let spawnpoint = race.spawnpoint_id.and_then(|id| {
        track
            .blueprints
            .iter()
            .find(|bp| bp.kind == "TrackBlueprintSpawnpoint" && bp.instance_id == id)
            .and_then(|bp| blueprint_prop(bp, "spawnpoint"))
    });
    let props = track
        .blueprints
        .iter()
        .filter(|bp| is_low_res_course_prop(&bp.item_id))
        .filter_map(|bp| blueprint_prop(bp, "prop"))
        .collect::<Vec<_>>();
    let collision_props = track
        .blueprints
        .iter()
        .filter(|bp| is_collision_course_prop(bp))
        .filter_map(|bp| blueprint_prop(bp, "collision_prop"))
        .collect::<Vec<_>>();
    let guide_path = build_guide_path(source, race, &checkpoints_by_id, spawnpoint.as_ref());

    let mut checkpoints = Vec::new();
    for (idx, passage) in ordered_passages(&race.passages).iter().enumerate() {
        let Some(bp) = checkpoints_by_id.get(&passage.checkpoint_id) else {
            continue;
        };
        let Some(position) = bp.position else {
            continue;
        };
        checkpoints.push(ReplayCheckpoint {
            sequence_index: idx as i64,
            checkpoint_id: passage.checkpoint_id,
            passage_type: passage.passage_type.clone(),
            directionality: passage.directionality.clone(),
            item_id: bp.item_id.clone(),
            position,
            rotation: bp.rotation.unwrap_or([0.0, 0.0, 0.0]),
            dimensions: bp.dimensions.unwrap_or([0.2, 2.0, 2.0]),
        });
    }

    if checkpoints.is_empty() && props.is_empty() {
        return Ok(None);
    }

    let course = ReplayCourseData {
        cache_id: cache_id.to_string(),
        game_title: source.game_title.clone(),
        data_root: path_string(&source.data_root),
        race_guid: race.guid.clone(),
        race_name: race.name.clone(),
        race_asset_key: race.asset_key.clone(),
        track_guid: Some(track.guid.clone()),
        track_name: Some(track.name.clone()),
        track_asset_key: track.asset_key.clone(),
        environment_id: track.environment_id.clone(),
        required_laps: race.required_laps,
        checkpoints,
        spawnpoint,
        props,
        collision_props,
        guide_path,
    };

    Ok(Some(RaceCourseCacheRow {
        cache_id: cache_id.to_string(),
        race_guid: race.guid.clone(),
        race_name: race.name.clone(),
        track_guid: Some(track.guid.clone()),
        track_name: Some(track.name.clone()),
        environment_id: track.environment_id.clone(),
        required_laps: race.required_laps,
        course_json: serde_json::to_string(&course)?,
    }))
}

fn source_status(
    source: &AssetSource,
    row: Option<&GameAssetCacheRow>,
) -> AppResult<GameAssetSourceStatus> {
    let root = path_string(&source.data_root);
    let (fingerprint_json, fingerprint_hash) = source_fingerprint(&source.data_root)?;
    let _ = fingerprint_json;
    let Some(row) = row else {
        return Ok(GameAssetSourceStatus {
            game_title: source.game_title.clone(),
            label: source.label.clone(),
            data_root: root,
            valid: true,
            cache_status: "missing".into(),
            cache_id: None,
            extracted_at: None,
            race_count: 0,
            track_count: 0,
            error_message: None,
        });
    };

    let cache_status = if row.extractor_version != EXTRACTOR_VERSION
        || row.source_fingerprint_hash != fingerprint_hash
    {
        "stale"
    } else if row.status == "error" {
        "error"
    } else {
        "fresh"
    };

    Ok(GameAssetSourceStatus {
        game_title: source.game_title.clone(),
        label: source.label.clone(),
        data_root: root,
        valid: true,
        cache_status: cache_status.into(),
        cache_id: Some(row.id.clone()),
        extracted_at: row.extracted_at,
        race_count: row.race_count,
        track_count: row.track_count,
        error_message: row.error_message.clone(),
    })
}

fn source_fingerprint(data_root: &Path) -> AppResult<(String, String)> {
    let fingerprint = SourceFingerprint {
        extractor_version: EXTRACTOR_VERSION.into(),
        data_root: path_string(data_root),
        files: source_file_fingerprints(data_root)?,
    };
    let json = serde_json::to_string(&fingerprint)?;
    let hash = hash_bytes(json.as_bytes());
    Ok((json, hash))
}

fn source_file_fingerprints(data_root: &Path) -> AppResult<Vec<SourceFileFingerprint>> {
    let mut paths = Vec::new();
    for name in ["resources.assets", "globalgamemanagers"] {
        let path = data_root.join(name);
        if path.exists() {
            paths.push(path);
        }
    }
    if let Ok(entries) = fs::read_dir(data_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("sharedassets") && name.ends_with(".assets") {
                paths.push(path);
            }
        }
    }
    let addressables_dir = data_root.join("StreamingAssets").join("aa");
    if addressables_dir.exists() {
        collect_addressable_fingerprint_files(&addressables_dir, &mut paths);
    }
    paths.sort();

    let mut out = Vec::new();
    for path in paths {
        let metadata = fs::metadata(&path)?;
        let modified_ms = metadata
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64);
        let relative_path = path
            .strip_prefix(data_root)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        out.push(SourceFileFingerprint {
            relative_path,
            size_bytes: metadata.len(),
            modified_ms,
        });
    }
    Ok(out)
}

fn collect_addressable_fingerprint_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_addressable_fingerprint_files(&path, out);
            continue;
        }

        let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
        if name == "settings.json"
            || (name.starts_with("catalog") && (name.ends_with(".bin") || name.ends_with(".json")))
        {
            out.push(path);
            continue;
        }

        if name.ends_with(".bundle") {
            out.push(path);
        }
    }
}

enum XmlItem {
    Track(TrackXml),
    Race(RaceXml),
}

fn extract_xml_items(data_root: &Path) -> AppResult<Vec<XmlItem>> {
    let mut sources = Vec::new();
    for name in ["resources.assets", "globalgamemanagers"] {
        let path = data_root.join(name);
        if path.exists() {
            sources.push(path);
        }
    }
    if let Ok(entries) = fs::read_dir(data_root) {
        for entry in entries.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.starts_with("sharedassets") && name.ends_with(".assets") {
                sources.push(path);
            }
        }
    }
    sources.sort();

    let mut items = Vec::new();
    for path in sources {
        let bytes = fs::read(&path)?;
        items.extend(extract_xml_items_from_embedded_bytes(&bytes));
    }
    items.extend(extract_xml_items_from_bundles(data_root));
    Ok(items)
}

fn extract_xml_items_from_embedded_bytes(bytes: &[u8]) -> Vec<XmlItem> {
    let mut items = Vec::new();
    let mut search_from = 0;
    while let Some(start) = find_bytes(&bytes[search_from..], XML_START) {
        let start = search_from + start;
        let window_end = (start + 512).min(bytes.len());
        let window = String::from_utf8_lossy(&bytes[start..window_end]);
        let track_pos = window.find("<Track");
        let race_pos = window.find("<Race");
        let (root, end_marker) = match (track_pos, race_pos) {
            (Some(t), Some(r)) if t < r => ("Track", TRACK_END),
            (Some(_), None) => ("Track", TRACK_END),
            (Some(_), Some(_)) | (None, Some(_)) => ("Race", RACE_END),
            (None, None) => {
                search_from = start + XML_START.len();
                continue;
            }
        };

        let Some(end) = find_bytes(&bytes[start..], end_marker.as_bytes()) else {
            search_from = start + XML_START.len();
            continue;
        };
        let xml_end = start + end + end_marker.len();
        let xml = String::from_utf8_lossy(&bytes[start..xml_end]).to_string();
        search_from = xml_end;
        push_xml_item(&xml, root, None, &mut items);
    }
    items
}

fn extract_xml_items_from_bundles(data_root: &Path) -> Vec<XmlItem> {
    let mut items = Vec::new();
    for path in addressable_xml_bundle_candidates(data_root) {
        let mut env = Environment::new();
        if env.load(&path).is_err() {
            continue;
        }

        for object in env.objects() {
            let EnvironmentObjectRef::Binary(object_ref) = object else {
                continue;
            };
            if object_ref.object.class_id() != 49 {
                continue;
            }
            let Ok(parsed) = object_ref.read() else {
                continue;
            };
            if parsed.class_name() != "TextAsset" {
                continue;
            }
            let Some(script) = parsed.get("m_Script").and_then(UnityValue::as_str) else {
                continue;
            };
            let asset_key = parsed
                .get("m_Name")
                .and_then(UnityValue::as_str)
                .map(str::trim)
                .filter(|name| !name.is_empty());
            if script.contains("<Track") {
                push_xml_item(script, "Track", asset_key, &mut items);
            } else if script.contains("<Race") {
                push_xml_item(script, "Race", asset_key, &mut items);
            }
        }
    }
    items
}

fn push_xml_item(xml: &str, root: &str, asset_key: Option<&str>, items: &mut Vec<XmlItem>) {
    if root == "Track" {
        if let Some(mut track) = parse_track_xml(xml) {
            track.asset_key = asset_key.map(str::to_string);
            items.push(XmlItem::Track(track));
        }
    } else if let Some(mut race) = parse_race_xml(xml) {
        race.asset_key = asset_key.map(str::to_string);
        items.push(XmlItem::Race(race));
    }
}

fn addressable_xml_bundle_candidates(data_root: &Path) -> Vec<PathBuf> {
    let root = data_root.join("StreamingAssets").join("aa");
    let mut candidates = Vec::new();
    collect_xml_bundle_candidates(&root, &mut candidates);
    candidates.sort();
    candidates
}

fn collect_xml_bundle_candidates(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_xml_bundle_candidates(&path, out);
            continue;
        }
        let is_small_bundle = path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name.ends_with(".bundle"))
            .unwrap_or(false)
            && fs::metadata(&path)
                .map(|metadata| metadata.len() <= MAX_XML_BUNDLE_SCAN_BYTES)
                .unwrap_or(false);
        if is_small_bundle {
            out.push(path);
        }
    }
}

fn parse_track_xml(xml: &str) -> Option<TrackXml> {
    let guid = local_id_guid(xml)?;
    let name = tag_text(xml, "name")?;
    let environment_id = tag_text(xml, "environment");
    let mut blueprints = Vec::new();

    for chunk in chunks(xml, "<TrackBlueprint", "</TrackBlueprint>") {
        let header_end = chunk.find('>').unwrap_or(chunk.len());
        let header = &chunk[..header_end];
        let kind = attr_value(header, "xsi:type")
            .or_else(|| attr_value(header, "type"))
            .unwrap_or_default();
        let item_id = tag_text(chunk, "itemID").unwrap_or_default();
        let instance_id = tag_text(chunk, "instanceID")
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(-1);
        let name = section_text(chunk, "spawnpoint").and_then(|section| tag_text(section, "name"));
        blueprints.push(TrackBlueprint {
            kind,
            item_id,
            instance_id,
            position: vec3_text(chunk, "position"),
            rotation: vec3_text(chunk, "rotation"),
            dimensions: vec3_text(chunk, "dimensions"),
            attach_points: ribbon_attach_points(chunk),
            name,
        });
    }

    Some(TrackXml {
        guid: normalize_guid(&guid),
        name,
        asset_key: None,
        environment_id,
        blueprints,
    })
}

fn parse_race_xml(xml: &str) -> Option<RaceXml> {
    let guid = local_id_guid(xml)?;
    let name = tag_text(xml, "name")?;
    let track_guid = first_dependency_guid(xml, "TRACK").map(|guid| normalize_guid(&guid));
    let required_laps = tag_text(xml, "requiredLaps").and_then(|v| v.parse::<i64>().ok());
    let spawnpoint_id = tag_text(xml, "spawnPointID").and_then(|v| v.parse::<i64>().ok());
    let mut passages = Vec::new();

    for chunk in chunks(xml, "<RaceCheckpointPassage>", "</RaceCheckpointPassage>") {
        let unique_id = tag_text(chunk, "uniqueId").unwrap_or_default();
        let checkpoint_id = tag_text(chunk, "checkPointID")
            .and_then(|value| value.parse::<i64>().ok())
            .unwrap_or(-1);
        let passage_type = tag_text(chunk, "passageType").unwrap_or_else(|| "Pass".into());
        let directionality = tag_text(chunk, "directionality").unwrap_or_else(|| "Any".into());
        let next_ids = section_text(chunk, "nextPassageIDs")
            .map(|section| {
                chunks(section, "<string>", "</string>")
                    .into_iter()
                    .map(|s| {
                        s.trim_start_matches("<string>")
                            .trim_end_matches("</string>")
                            .trim()
                            .to_string()
                    })
                    .filter(|s| !s.is_empty())
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        passages.push(RacePassage {
            unique_id,
            checkpoint_id,
            passage_type,
            directionality,
            next_ids,
        });
    }

    Some(RaceXml {
        guid: normalize_guid(&guid),
        name,
        asset_key: None,
        track_guid,
        required_laps,
        spawnpoint_id,
        passages,
    })
}

fn ordered_passages(passages: &[RacePassage]) -> Vec<RacePassage> {
    if passages.is_empty() {
        return Vec::new();
    }
    let by_id: HashMap<&str, &RacePassage> =
        passages.iter().map(|p| (p.unique_id.as_str(), p)).collect();
    let mut used = HashSet::new();
    let mut ordered = Vec::new();

    let mut current = passages
        .iter()
        .find(|p| p.passage_type.eq_ignore_ascii_case("start"))
        .unwrap_or(&passages[0]);

    loop {
        if !used.insert(current.unique_id.clone()) {
            break;
        }
        ordered.push(current.clone());
        let Some(next_id) = current.next_ids.first() else {
            break;
        };
        let Some(next) = by_id.get(next_id.as_str()) else {
            break;
        };
        current = next;
    }

    for passage in passages {
        if passage.passage_type.eq_ignore_ascii_case("finish") && !used.contains(&passage.unique_id)
        {
            ordered.push(passage.clone());
        }
    }

    if ordered.is_empty() {
        passages.to_vec()
    } else {
        ordered
    }
}

fn blueprint_prop(bp: &TrackBlueprint, kind: &str) -> Option<ReplayCourseProp> {
    Some(ReplayCourseProp {
        instance_id: bp.instance_id,
        item_id: bp.item_id.clone(),
        kind: kind.into(),
        position: bp.position?,
        rotation: bp.rotation.unwrap_or([0.0, 0.0, 0.0]),
        dimensions: bp.dimensions,
        attach_points: bp.attach_points.clone(),
        procedural_geometry: is_procedural_ribbon_blueprint(bp),
        name: bp.name.clone(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Vec3 {
    x: f32,
    y: f32,
    z: f32,
}

impl Vec3 {
    fn new(x: f32, y: f32, z: f32) -> Self {
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

    fn mul(self, scalar: f32) -> Self {
        Self::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }

    fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y + self.z * other.z
    }

    fn magnitude(self) -> f32 {
        self.dot(self).sqrt()
    }

    fn normalized(self) -> Self {
        let magnitude = self.magnitude();
        if magnitude <= f32::EPSILON {
            Self::new(0.0, 0.0, 0.0)
        } else {
            self.mul(1.0 / magnitude)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum GuideVariant {
    Liftoff,
    MicroDrones,
}

fn build_guide_path(
    source: &AssetSource,
    race: &RaceXml,
    checkpoints_by_id: &HashMap<i64, &TrackBlueprint>,
    spawnpoint: Option<&ReplayCourseProp>,
) -> Option<ReplayGuidePath> {
    if race.passages.is_empty() {
        return None;
    }

    let by_id: HashMap<&str, &RacePassage> = race
        .passages
        .iter()
        .map(|p| (p.unique_id.as_str(), p))
        .collect();
    let start = race
        .passages
        .iter()
        .find(|p| p.passage_type.eq_ignore_ascii_case("start"))
        .unwrap_or(&race.passages[0]);
    let variant = guide_variant_for_source(source);
    let mut segments = Vec::new();
    let mut entry_direction = checkpoint_forward(start, checkpoints_by_id, variant);

    if let Some(spawnpoint) = spawnpoint {
        if let Some(start_bp) = checkpoints_by_id.get(&start.checkpoint_id) {
            if let Some(start_position) = start_bp.position.map(Vec3::from_array) {
                let spawn_position = Vec3::from_array(spawnpoint.position);
                let spawn_forward =
                    rotate_euler_zxy(Vec3::new(0.0, 0.0, 1.0), spawnpoint.rotation).normalized();
                entry_direction = spawn_forward;
                let points = fallback_guide_segment(spawn_position, start_position, spawn_forward);
                if points.len() >= 2 {
                    entry_direction = segment_exit_direction(&points).unwrap_or(entry_direction);
                    segments.push(ReplayGuidePathSegment {
                        from_passage_id: None,
                        to_passage_id: Some(start.unique_id.clone()),
                        from_checkpoint_id: None,
                        to_checkpoint_id: Some(start.checkpoint_id),
                        points,
                    });
                }
            }
        }
    }

    let mut visited_edges = HashSet::new();
    collect_guide_segments(
        start,
        entry_direction,
        &by_id,
        checkpoints_by_id,
        variant,
        &mut visited_edges,
        &mut segments,
    );

    if segments.is_empty() {
        None
    } else {
        Some(ReplayGuidePath {
            algorithm: match variant {
                GuideVariant::Liftoff => "liftoff-flexible-box-v2".into(),
                GuideVariant::MicroDrones => "liftoff-micro-flexible-box-v2".into(),
            },
            accuracy: "approximate".into(),
            segments,
        })
    }
}

fn collect_guide_segments(
    current: &RacePassage,
    entry_direction: Vec3,
    by_id: &HashMap<&str, &RacePassage>,
    checkpoints_by_id: &HashMap<i64, &TrackBlueprint>,
    variant: GuideVariant,
    visited_edges: &mut HashSet<(String, String)>,
    segments: &mut Vec<ReplayGuidePathSegment>,
) {
    if current.passage_type.eq_ignore_ascii_case("finish") || visited_edges.len() > 512 {
        return;
    }

    for next_id in &current.next_ids {
        if !visited_edges.insert((current.unique_id.clone(), next_id.clone())) {
            continue;
        }
        let Some(next) = by_id.get(next_id.as_str()).copied() else {
            continue;
        };
        let Some(points) =
            checkpoint_guide_segment(current, next, entry_direction, checkpoints_by_id, variant)
        else {
            continue;
        };
        if points.len() < 2 {
            continue;
        }
        let exit_direction = segment_exit_direction(&points).unwrap_or(entry_direction);
        segments.push(ReplayGuidePathSegment {
            from_passage_id: Some(current.unique_id.clone()),
            to_passage_id: Some(next.unique_id.clone()),
            from_checkpoint_id: Some(current.checkpoint_id),
            to_checkpoint_id: Some(next.checkpoint_id),
            points,
        });
        collect_guide_segments(
            next,
            exit_direction,
            by_id,
            checkpoints_by_id,
            variant,
            visited_edges,
            segments,
        );
    }
}

fn checkpoint_guide_segment(
    from: &RacePassage,
    to: &RacePassage,
    entry_direction: Vec3,
    checkpoints_by_id: &HashMap<i64, &TrackBlueprint>,
    variant: GuideVariant,
) -> Option<Vec<[f32; 3]>> {
    let from_bp = checkpoints_by_id.get(&from.checkpoint_id)?;
    let to_bp = checkpoints_by_id.get(&to.checkpoint_id)?;
    let from_position = from_bp.position.map(Vec3::from_array)?;
    let to_position = to_bp.position.map(Vec3::from_array)?;
    let distance = to_position.sub(from_position).magnitude();

    let Some(from_guides) = flexible_checkpoint_guide_points(from_bp, variant) else {
        return Some(fallback_guide_segment(
            from_position,
            to_position,
            entry_direction,
        ));
    };
    let Some(to_guides) = flexible_checkpoint_guide_points(to_bp, variant) else {
        return Some(fallback_guide_segment(
            from_position,
            to_position,
            entry_direction,
        ));
    };

    let source_guide = choose_source_guide(from_position, from_guides, entry_direction);
    let points = match variant {
        GuideVariant::Liftoff => {
            let target_guide = nearest_guide_point(source_guide, to_guides);
            let start_control = from_position.add(
                source_guide
                    .sub(from_position)
                    .normalized()
                    .mul(distance * 0.33),
            );
            let end_control = to_position.add(
                target_guide
                    .sub(to_position)
                    .normalized()
                    .mul(distance * 0.33),
            );
            sample_cubic_guide(from_position, start_control, end_control, to_position)
        }
        GuideVariant::MicroDrones => {
            let start_control = scaled_micro_guide_point(from_position, source_guide, distance);
            let to_scaled = [
                scaled_micro_guide_point(to_position, to_guides[0], distance),
                scaled_micro_guide_point(to_position, to_guides[1], distance),
            ];
            let end_control = nearest_guide_point(start_control, to_scaled);
            sample_cubic_guide(from_position, start_control, end_control, to_position)
        }
    };
    Some(points)
}

fn guide_variant_for_source(source: &AssetSource) -> GuideVariant {
    if normalize_key(&source.game_title).contains("microdrones") {
        GuideVariant::MicroDrones
    } else {
        GuideVariant::Liftoff
    }
}

fn checkpoint_forward(
    passage: &RacePassage,
    checkpoints_by_id: &HashMap<i64, &TrackBlueprint>,
    variant: GuideVariant,
) -> Vec3 {
    checkpoints_by_id
        .get(&passage.checkpoint_id)
        .map(|bp| {
            rotate_euler_zxy(
                checkpoint_forward_axis(bp, variant),
                bp.rotation.unwrap_or([0.0; 3]),
            )
        })
        .unwrap_or_else(|| Vec3::new(0.0, 0.0, 1.0))
        .normalized()
}

fn checkpoint_forward_axis(bp: &TrackBlueprint, variant: GuideVariant) -> Vec3 {
    let [x, y, z] = checkpoint_local_dimensions(bp, variant);
    if x <= y && x <= z {
        Vec3::new(1.0, 0.0, 0.0)
    } else if y <= x && y <= z {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    }
}

fn flexible_checkpoint_guide_points(
    bp: &TrackBlueprint,
    variant: GuideVariant,
) -> Option<[Vec3; 2]> {
    let position = Vec3::from_array(bp.position?);
    let rotation = bp.rotation.unwrap_or([0.0, 0.0, 0.0]);
    let [x, y, z] = checkpoint_local_dimensions(bp, variant);
    let magnitude = (x * x + y * y + z * z).sqrt();
    if magnitude <= f32::EPSILON {
        return None;
    }

    let face_x = y * z / magnitude;
    let face_y = x * z / magnitude;
    let face_z = x * y / magnitude;
    let (positive, negative) = if face_x >= face_y && face_x >= face_z {
        (Vec3::new(face_x, 0.0, 0.0), Vec3::new(-face_x, 0.0, 0.0))
    } else if face_y >= face_x && face_y >= face_z {
        (Vec3::new(0.0, face_y, 0.0), Vec3::new(0.0, -face_y, 0.0))
    } else {
        (Vec3::new(0.0, 0.0, face_z), Vec3::new(0.0, 0.0, -face_z))
    };

    Some([
        position.add(rotate_euler_zxy(positive, rotation)),
        position.add(rotate_euler_zxy(negative, rotation)),
    ])
}

fn checkpoint_local_dimensions(bp: &TrackBlueprint, variant: GuideVariant) -> [f32; 3] {
    let [x, y, z] = bp.dimensions.unwrap_or([0.2, 2.0, 2.0]);
    match variant {
        GuideVariant::Liftoff => [z.abs(), y.abs(), x.abs()],
        GuideVariant::MicroDrones => [x.abs(), y.abs(), z.abs()],
    }
}

fn choose_source_guide(position: Vec3, guides: [Vec3; 2], entry_direction: Vec3) -> Vec3 {
    let entry = entry_direction.normalized();
    if entry.magnitude() <= f32::EPSILON {
        return guides[0];
    }
    guides
        .into_iter()
        .max_by(|a, b| {
            let a_score = a.sub(position).normalized().dot(entry);
            let b_score = b.sub(position).normalized().dot(entry);
            a_score
                .partial_cmp(&b_score)
                .unwrap_or(std::cmp::Ordering::Equal)
        })
        .unwrap_or(guides[0])
}

fn nearest_guide_point(anchor: Vec3, guides: [Vec3; 2]) -> Vec3 {
    let first = guides[0].sub(anchor).magnitude();
    let second = guides[1].sub(anchor).magnitude();
    if first <= second {
        guides[0]
    } else {
        guides[1]
    }
}

fn scaled_micro_guide_point(position: Vec3, guide: Vec3, distance: f32) -> Vec3 {
    guide.add(guide.sub(position).normalized().mul(distance * 0.2))
}

fn fallback_guide_segment(start: Vec3, end: Vec3, entry_direction: Vec3) -> Vec<[f32; 3]> {
    let distance = end.sub(start).magnitude();
    if distance <= f32::EPSILON {
        return vec![apply_guide_vertical_offset(start).to_array()];
    }
    let route_direction = end.sub(start).normalized();
    let entry = if entry_direction.magnitude() <= f32::EPSILON {
        route_direction
    } else {
        entry_direction.normalized()
    };
    let start_control = start.add(entry.mul(distance * 0.35));
    let end_control = end.sub(route_direction.mul(distance * 0.25));
    sample_cubic_guide(start, start_control, end_control, end)
}

fn sample_cubic_guide(start: Vec3, control_a: Vec3, control_b: Vec3, end: Vec3) -> Vec<[f32; 3]> {
    let distance = end.sub(start).magnitude();
    let samples = (distance.ceil() as usize).clamp(8, 128);
    let mut points = Vec::with_capacity(samples + 1);
    for idx in 0..=samples {
        let t = idx as f32 / samples as f32;
        points.push(
            apply_guide_vertical_offset(cubic_point(start, control_a, control_b, end, t))
                .to_array(),
        );
    }
    points
}

fn cubic_point(start: Vec3, control_a: Vec3, control_b: Vec3, end: Vec3, t: f32) -> Vec3 {
    let inv = 1.0 - t;
    start
        .mul(inv * inv * inv)
        .add(control_a.mul(3.0 * inv * inv * t))
        .add(control_b.mul(3.0 * inv * t * t))
        .add(end.mul(t * t * t))
}

fn apply_guide_vertical_offset(point: Vec3) -> Vec3 {
    point.add(Vec3::new(0.0, -0.1, 0.0))
}

fn segment_exit_direction(points: &[[f32; 3]]) -> Option<Vec3> {
    let end = Vec3::from_array(*points.last()?);
    let start = Vec3::from_array(*points.get(points.len().saturating_sub(2))?);
    let direction = end.sub(start).normalized();
    if direction.magnitude() <= f32::EPSILON {
        None
    } else {
        Some(direction)
    }
}

fn rotate_euler_zxy(value: Vec3, degrees: [f32; 3]) -> Vec3 {
    let [x, y, z] = degrees.map(f32::to_radians);
    rotate_y(rotate_x(rotate_z(value, z), x), y)
}

fn rotate_x(value: Vec3, radians: f32) -> Vec3 {
    let (sin, cos) = radians.sin_cos();
    Vec3::new(
        value.x,
        value.y * cos - value.z * sin,
        value.y * sin + value.z * cos,
    )
}

fn rotate_y(value: Vec3, radians: f32) -> Vec3 {
    let (sin, cos) = radians.sin_cos();
    Vec3::new(
        value.x * cos + value.z * sin,
        value.y,
        -value.x * sin + value.z * cos,
    )
}

fn rotate_z(value: Vec3, radians: f32) -> Vec3 {
    let (sin, cos) = radians.sin_cos();
    Vec3::new(
        value.x * cos - value.y * sin,
        value.x * sin + value.y * cos,
        value.z,
    )
}

fn checkpoint_blueprints_by_id(blueprints: &[TrackBlueprint]) -> HashMap<i64, &TrackBlueprint> {
    let mut out = HashMap::new();
    for bp in blueprints {
        if bp.instance_id < 0 || bp.position.is_none() {
            continue;
        }
        if out.contains_key(&bp.instance_id) && bp.kind != "TrackBlueprintFlexibleCheckpoint" {
            continue;
        }
        out.insert(bp.instance_id, bp);
    }
    out
}

fn is_low_res_course_prop(item_id: &str) -> bool {
    let id = item_id.to_ascii_lowercase();
    id.contains("gate")
        || id.contains("checkpoint")
        || id.contains("finish")
        || id.contains("balloon")
        || id.contains("arrow")
        || id.contains("cone")
        || id.contains("flag")
}

fn is_collision_course_prop(bp: &TrackBlueprint) -> bool {
    if bp.position.is_none() || bp.item_id.trim().is_empty() {
        return false;
    }
    let kind = bp.kind.to_ascii_lowercase();
    let id = bp.item_id.to_ascii_lowercase();
    if kind.contains("spawnpoint")
        || kind.contains("checkpoint")
        || id.contains("checkpoint")
        || id.contains("trigger")
    {
        return false;
    }
    true
}

fn geometry_scopes_for_tracks_and_races<'a>(
    tracks: impl IntoIterator<Item = &'a TrackXml>,
    races: impl IntoIterator<Item = &'a RaceXml>,
) -> Vec<GeometryScope> {
    let mut scopes = BTreeSet::new();
    for race in races {
        if let Some(asset_key) = &race.asset_key {
            if !asset_key.trim().is_empty() {
                scopes.insert(GeometryScope::race(asset_key.clone()));
            }
        }
    }
    for track in tracks {
        if let Some(environment_id) = &track.environment_id {
            if !environment_id.trim().is_empty() {
                scopes.insert(GeometryScope::environment(environment_id.clone()));
            }
        }
        for bp in &track.blueprints {
            if is_collision_course_prop(bp) {
                if !is_procedural_ribbon_blueprint(bp) {
                    scopes.insert(GeometryScope::item(bp.item_id.clone()));
                }
            }
        }
    }
    scopes.into_iter().collect()
}

fn is_procedural_ribbon_blueprint(bp: &TrackBlueprint) -> bool {
    bp.kind.to_ascii_lowercase().contains("ribbon")
}

fn course_match_score(
    session: &RaceSessionRow,
    cache: &GameAssetCacheRow,
    course: &RaceCourseCacheRow,
) -> i32 {
    let mut score = 0;
    let session_title = session.title.as_deref().map(normalize_key);
    let cache_title = normalize_key(&cache.game_title);
    if let Some(title) = &session_title {
        if *title == cache_title {
            score += 25;
        }
    }

    if let Some(guid) = session.race_guid.as_deref().map(normalize_guid) {
        if guid == normalize_guid(&course.race_guid) {
            score += 100;
        } else if session.race.is_none() && session.track.is_none() {
            return 0;
        }
    }

    if let Some(race) = &session.race {
        if normalize_key(race) == normalize_key(&course.race_name) {
            score += 45;
        }
    }
    if let (Some(track), Some(course_track)) = (&session.track, &course.track_name) {
        if normalize_key(track) == normalize_key(course_track) {
            score += 25;
        }
    }
    if let Some(level) = &session.level {
        if let Some(env) = &course.environment_id {
            if normalize_key(level) == normalize_key(env) {
                score += 5;
            }
        }
    }

    score
}

fn manual_source(path: &str) -> AppResult<AssetSource> {
    let input = PathBuf::from(path).expand_home();
    let data_root = normalize_data_root(&input).ok_or_else(|| {
        AppError::LiftoffConfig(format!(
            "could not find Unity Data files below {}",
            input.to_string_lossy()
        ))
    })?;
    let game_title = game_title_from_name("", &data_root);
    Ok(AssetSource {
        game_title,
        label: "Manual path".into(),
        data_root,
    })
}

fn push_valid_source(
    out: &mut Vec<AssetSource>,
    seen: &mut HashSet<String>,
    title: impl Into<String>,
    label: impl Into<String>,
    path: &Path,
) {
    if let Some(data_root) = normalize_data_root(path) {
        let key = path_string(&data_root);
        if seen.insert(key) {
            out.push(AssetSource {
                game_title: title.into(),
                label: label.into(),
                data_root,
            });
        }
    }
}

fn normalize_data_root(path: &Path) -> Option<PathBuf> {
    let candidates = [
        path.to_path_buf(),
        path.join("Data"),
        path.join("Contents").join("Resources").join("Data"),
        path.join("Liftoff.app")
            .join("Contents")
            .join("Resources")
            .join("Data"),
        path.join("Liftoff Micro Drones.app")
            .join("Contents")
            .join("Resources")
            .join("Data"),
        path.join("Liftoff_Data"),
        path.join("Liftoff Micro Drones_Data"),
    ];
    for candidate in candidates {
        if is_unity_data_root(&candidate) {
            return Some(canonical_or_original(candidate));
        }
    }

    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let child = entry.path();
            let name = child.file_name().and_then(|n| n.to_str()).unwrap_or("");
            if name.ends_with(".app") {
                let data = child.join("Contents").join("Resources").join("Data");
                if is_unity_data_root(&data) {
                    return Some(canonical_or_original(data));
                }
            }
            if name.ends_with("_Data") && is_unity_data_root(&child) {
                return Some(canonical_or_original(child));
            }
        }
    }

    None
}

fn is_unity_data_root(path: &Path) -> bool {
    path.join("resources.assets").is_file()
}

fn canonical_or_original(path: PathBuf) -> PathBuf {
    path.canonicalize().unwrap_or(path)
}

fn known_install_candidates() -> Vec<(&'static str, &'static str, PathBuf)> {
    let mut out = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "macos")]
        {
            let common = home.join("Library/Application Support/Steam/steamapps/common");
            out.push(("Liftoff", "macOS Steam · Liftoff", common.join("Liftoff")));
            out.push((
                "Liftoff Micro Drones",
                "macOS Steam · Liftoff Micro Drones",
                common.join("Liftoff Micro Drones"),
            ));
        }
        #[cfg(target_os = "linux")]
        {
            for common in [
                home.join(".steam/steam/steamapps/common"),
                home.join(".local/share/Steam/steamapps/common"),
                home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam/steamapps/common"),
            ] {
                out.push(("Liftoff", "Linux Steam · Liftoff", common.join("Liftoff")));
                out.push((
                    "Liftoff Micro Drones",
                    "Linux Steam · Liftoff Micro Drones",
                    common.join("Liftoff Micro Drones"),
                ));
            }
        }
    }
    #[cfg(target_os = "windows")]
    {
        for base in [
            std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from),
            std::env::var_os("PROGRAMFILES").map(PathBuf::from),
        ]
        .into_iter()
        .flatten()
        {
            let common = base.join("Steam").join("steamapps").join("common");
            out.push(("Liftoff", "Windows Steam · Liftoff", common.join("Liftoff")));
            out.push((
                "Liftoff Micro Drones",
                "Windows Steam · Liftoff Micro Drones",
                common.join("Liftoff Micro Drones"),
            ));
        }
    }
    out
}

fn steam_libraries() -> Vec<PathBuf> {
    let mut roots = Vec::new();
    if let Some(home) = dirs::home_dir() {
        #[cfg(target_os = "macos")]
        roots.push(home.join("Library/Application Support/Steam"));
        #[cfg(target_os = "linux")]
        {
            roots.push(home.join(".steam/steam"));
            roots.push(home.join(".local/share/Steam"));
            roots.push(home.join(".var/app/com.valvesoftware.Steam/.local/share/Steam"));
        }
    }
    #[cfg(target_os = "windows")]
    {
        for base in [
            std::env::var_os("PROGRAMFILES(X86)").map(PathBuf::from),
            std::env::var_os("PROGRAMFILES").map(PathBuf::from),
        ]
        .into_iter()
        .flatten()
        {
            roots.push(base.join("Steam"));
        }
    }

    let mut libraries = HashSet::new();
    for root in roots {
        if root.exists() {
            libraries.insert(canonical_or_original(root.clone()));
        }
        let vdf = root.join("steamapps").join("libraryfolders.vdf");
        let Ok(text) = fs::read_to_string(vdf) else {
            continue;
        };
        for path in extract_vdf_paths(&text) {
            let path = PathBuf::from(path);
            if path.exists() {
                libraries.insert(canonical_or_original(path));
            }
        }
    }
    libraries.into_iter().collect()
}

fn extract_vdf_paths(text: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for line in text.lines() {
        let pairs = quoted_fields(line);
        if pairs.len() < 2 {
            continue;
        }
        let key = &pairs[0];
        let value = &pairs[1];
        if key == "path" || (key.chars().all(|c| c.is_ascii_digit()) && looks_like_path(value)) {
            paths.push(value.replace("\\\\", "\\"));
        }
    }
    paths
}

fn extract_vdf_value(text: &str, wanted: &str) -> Option<String> {
    for line in text.lines() {
        let pairs = quoted_fields(line);
        if pairs.len() >= 2 && pairs[0] == wanted {
            return Some(pairs[1].clone());
        }
    }
    None
}

fn quoted_fields(line: &str) -> Vec<String> {
    let mut fields = Vec::new();
    let mut in_quote = false;
    let mut escaped = false;
    let mut current = String::new();
    for ch in line.chars() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' && in_quote {
            escaped = true;
            continue;
        }
        if ch == '"' {
            if in_quote {
                fields.push(current.clone());
                current.clear();
            }
            in_quote = !in_quote;
            continue;
        }
        if in_quote {
            current.push(ch);
        }
    }
    fields
}

fn looks_like_path(value: &str) -> bool {
    value.contains('/') || value.contains('\\') || value.contains(':')
}

fn is_liftoff_game_name(name: &str) -> bool {
    let normalized = normalize_key(name);
    normalized == "liftoff" || normalized.contains("liftoffmicrodrones")
}

fn game_title_from_name(name: &str, path: &Path) -> String {
    let haystack = format!("{} {}", name, path.to_string_lossy());
    if normalize_key(&haystack).contains("microdrones") {
        "Liftoff Micro Drones".into()
    } else {
        "Liftoff".into()
    }
}

fn local_id_guid(xml: &str) -> Option<String> {
    section_text(xml, "localID").and_then(first_guid_tag)
}

fn first_dependency_guid(xml: &str, wanted_type: &str) -> Option<String> {
    for dep in chunks(xml, "<dependency>", "</dependency>") {
        if tag_text(dep, "type").as_deref() == Some(wanted_type) {
            return first_guid_tag(dep);
        }
    }
    None
}

fn first_guid_tag(xml: &str) -> Option<String> {
    tag_text(xml, "str").or_else(|| tag_text(xml, "guid"))
}

fn tag_text(text: &str, tag: &str) -> Option<String> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let tag_start = text.find(&open)?;
    let start = text[tag_start..].find('>')? + tag_start + 1;
    let end = text[start..].find(&close)? + start;
    Some(unescape_basic_xml(text[start..end].trim()))
}

fn section_text<'a>(text: &'a str, tag: &str) -> Option<&'a str> {
    let open = format!("<{}", tag);
    let close = format!("</{}>", tag);
    let tag_start = text.find(&open)?;
    let start = text[tag_start..].find('>')? + tag_start + 1;
    let end = text[start..].find(&close)? + start;
    Some(&text[start..end])
}

fn chunks<'a>(text: &'a str, start_marker: &str, end_marker: &str) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut search_from = 0;
    while let Some(start) = text[search_from..].find(start_marker) {
        let start = search_from + start;
        let Some(end) = text[start..].find(end_marker) else {
            break;
        };
        let end = start + end + end_marker.len();
        out.push(&text[start..end]);
        search_from = end;
    }
    out
}

fn attr_value(text: &str, name: &str) -> Option<String> {
    let marker = format!("{}=\"", name);
    let start = text.find(&marker)? + marker.len();
    let end = text[start..].find('"')? + start;
    Some(unescape_basic_xml(&text[start..end]))
}

fn vec3_text(text: &str, tag: &str) -> Option<[f32; 3]> {
    let section = section_text(text, tag)?;
    Some([
        tag_text(section, "x")?.parse().ok()?,
        tag_text(section, "y")?.parse().ok()?,
        tag_text(section, "z")?.parse().ok()?,
    ])
}

fn ribbon_attach_points(text: &str) -> Vec<[f32; 3]> {
    let Some(section) = section_text(text, "attachPoints") else {
        return Vec::new();
    };
    chunks(section, "<RibbonAttachPoint", "</RibbonAttachPoint>")
        .into_iter()
        .filter_map(|point| vec3_text(point, "position"))
        .collect()
}

fn unescape_basic_xml(value: &str) -> String {
    value
        .replace("&apos;", "'")
        .replace("&quot;", "\"")
        .replace("&gt;", ">")
        .replace("&lt;", "<")
        .replace("&amp;", "&")
}

fn normalize_guid(value: &str) -> String {
    value.trim().to_ascii_lowercase()
}

fn normalize_key(value: &str) -> String {
    value
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .flat_map(|c| c.to_lowercase())
        .collect()
}

fn cache_id_for_root(data_root: &Path) -> String {
    let hash = hash_bytes(path_string(data_root).as_bytes());
    format!("assets_{}", &hash[..16])
}

fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    haystack.windows(needle.len()).position(|w| w == needle)
}

trait ExpandHome {
    fn expand_home(self) -> PathBuf;
}

impl ExpandHome for PathBuf {
    fn expand_home(self) -> PathBuf {
        let s = self.to_string_lossy();
        if let Some(rest) = s.strip_prefix("~/") {
            if let Some(home) = dirs::home_dir() {
                return home.join(rest);
            }
        }
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_close(actual: f32, expected: f32) {
        assert!(
            (actual - expected).abs() < 0.001,
            "expected {actual} to be close to {expected}"
        );
    }

    fn test_source(game_title: &str) -> AssetSource {
        AssetSource {
            game_title: game_title.into(),
            label: "Test".into(),
            data_root: PathBuf::from("/tmp/liftoff-test"),
        }
    }

    fn test_checkpoint(instance_id: i64, position: [f32; 3]) -> TrackBlueprint {
        TrackBlueprint {
            kind: "TrackBlueprintFlexibleCheckpoint".into(),
            item_id: "CheckpointBoxFlexible01".into(),
            instance_id,
            position: Some(position),
            rotation: Some([0.0, 0.0, 0.0]),
            dimensions: Some([0.2, 2.0, 2.0]),
            attach_points: Vec::new(),
            name: None,
        }
    }

    #[test]
    fn parses_track_checkpoint_blueprints() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<Track xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <localID><str>track-guid</str><version>1</version><type>TRACK</type></localID>
  <name>01 - Test</name>
  <environment>TestEnv</environment>
  <blueprints>
    <TrackBlueprint xsi:type="TrackBlueprintFlexibleCheckpoint">
      <itemID>CheckpointBoxFlexible01</itemID>
      <instanceID>2</instanceID>
      <position><x>1</x><y>2</y><z>3</z></position>
      <rotation><x>0</x><y>90</y><z>0</z></rotation>
      <dimensions><x>0.1</x><y>3</y><z>2.4</z></dimensions>
    </TrackBlueprint>
  </blueprints>
</Track>"#;
        let track = parse_track_xml(xml).unwrap();
        assert_eq!(track.guid, "track-guid");
        assert_eq!(track.blueprints.len(), 1);
        assert_eq!(track.blueprints[0].position, Some([1.0, 2.0, 3.0]));
        assert_eq!(track.blueprints[0].dimensions, Some([0.1, 3.0, 2.4]));
    }

    #[test]
    fn parses_ribbon_attach_points() {
        let xml = r#"<?xml version="1.0" encoding="utf-8"?>
<Track xmlns:xsi="http://www.w3.org/2001/XMLSchema-instance">
  <localID><str>track-guid</str><version>1</version><type>TRACK</type></localID>
  <name>01 - Test</name>
  <blueprints>
    <TrackBlueprint xsi:type="TrackBlueprintRibbon">
      <itemID>TapeWhiteRed01</itemID>
      <instanceID>-1</instanceID>
      <position><x>0</x><y>0</y><z>0</z></position>
      <rotation><x>0</x><y>0</y><z>0</z></rotation>
      <attachPoints>
        <RibbonAttachPoint>
          <position><x>1</x><y>2</y><z>3</z></position>
        </RibbonAttachPoint>
        <RibbonAttachPoint>
          <position><x>4</x><y>5</y><z>6</z></position>
        </RibbonAttachPoint>
      </attachPoints>
    </TrackBlueprint>
  </blueprints>
</Track>"#;
        let track = parse_track_xml(xml).unwrap();
        assert_eq!(
            track.blueprints[0].attach_points,
            vec![[1.0, 2.0, 3.0], [4.0, 5.0, 6.0]]
        );
    }

    #[test]
    fn flexible_box_guide_points_use_rendered_gate_local_axes() {
        let checkpoint = test_checkpoint(1, [1.0, 2.0, 3.0]);
        let points = flexible_checkpoint_guide_points(&checkpoint, GuideVariant::Liftoff).unwrap();
        let expected_offset = 4.0 / (0.2_f32 * 0.2 + 2.0 * 2.0 + 2.0 * 2.0).sqrt();

        assert_close(points[0].x, 1.0);
        assert_close(points[0].y, 2.0);
        assert_close(points[0].z, 3.0 + expected_offset);
        assert_close(points[1].z, 3.0 - expected_offset);
    }

    #[test]
    fn micro_flexible_box_guide_points_use_raw_local_axes() {
        let mut checkpoint = test_checkpoint(1, [1.0, 2.0, 3.0]);
        checkpoint.dimensions = Some([0.05, 1.1, 3.95]);

        let points =
            flexible_checkpoint_guide_points(&checkpoint, GuideVariant::MicroDrones).unwrap();
        let expected_offset = 1.1 * 3.95 / (0.05_f32 * 0.05 + 1.1 * 1.1 + 3.95 * 3.95).sqrt();

        assert_close(points[0].x, 1.0 + expected_offset);
        assert_close(points[0].y, 2.0);
        assert_close(points[0].z, 3.0);
        assert_close(points[1].x, 1.0 - expected_offset);
    }

    #[test]
    fn orders_race_passages_from_start_graph() {
        let passages = vec![
            RacePassage {
                unique_id: "finish".into(),
                checkpoint_id: 1,
                passage_type: "Finish".into(),
                directionality: "RightToLeft".into(),
                next_ids: vec![],
            },
            RacePassage {
                unique_id: "start".into(),
                checkpoint_id: 1,
                passage_type: "Start".into(),
                directionality: "RightToLeft".into(),
                next_ids: vec!["middle".into()],
            },
            RacePassage {
                unique_id: "middle".into(),
                checkpoint_id: 2,
                passage_type: "Pass".into(),
                directionality: "RightToLeft".into(),
                next_ids: vec![],
            },
        ];
        let ordered = ordered_passages(&passages);
        assert_eq!(ordered[0].unique_id, "start");
        assert_eq!(ordered[1].unique_id, "middle");
        assert_eq!(ordered[2].unique_id, "finish");
    }

    #[test]
    fn build_course_emits_guide_path_from_race_graph() {
        let source = test_source("Liftoff");
        let track = TrackXml {
            guid: "track-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            environment_id: Some("TestEnv".into()),
            blueprints: vec![
                test_checkpoint(1, [0.0, 1.0, 0.0]),
                test_checkpoint(2, [0.0, 1.0, 10.0]),
                test_checkpoint(3, [0.0, 1.0, 20.0]),
            ],
        };
        let race = RaceXml {
            guid: "race-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            track_guid: Some("track-guid".into()),
            required_laps: Some(1),
            spawnpoint_id: None,
            passages: vec![
                RacePassage {
                    unique_id: "finish".into(),
                    checkpoint_id: 3,
                    passage_type: "Finish".into(),
                    directionality: "Any".into(),
                    next_ids: vec![],
                },
                RacePassage {
                    unique_id: "start".into(),
                    checkpoint_id: 1,
                    passage_type: "Start".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["middle".into()],
                },
                RacePassage {
                    unique_id: "middle".into(),
                    checkpoint_id: 2,
                    passage_type: "Pass".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["finish".into()],
                },
            ],
        };
        let tracks = HashMap::from([(track.guid.clone(), track)]);
        let row = build_course("cache", &source, &race, &tracks)
            .unwrap()
            .unwrap();
        let course: ReplayCourseData = serde_json::from_str(&row.course_json).unwrap();
        let guide_path = course.guide_path.unwrap();

        assert_eq!(guide_path.accuracy, "approximate");
        assert_eq!(guide_path.segments.len(), 2);
        assert_eq!(
            guide_path.segments[0].from_passage_id.as_deref(),
            Some("start")
        );
        assert_eq!(
            guide_path.segments[0].to_passage_id.as_deref(),
            Some("middle")
        );
        assert!(!guide_path.segments[0].points.is_empty());
    }

    #[test]
    fn guide_path_traverses_branches_without_infinite_cycles() {
        let source = test_source("Liftoff");
        let track = TrackXml {
            guid: "track-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            environment_id: None,
            blueprints: vec![
                test_checkpoint(1, [0.0, 1.0, 0.0]),
                test_checkpoint(2, [-5.0, 1.0, 10.0]),
                test_checkpoint(3, [5.0, 1.0, 10.0]),
                test_checkpoint(4, [0.0, 1.0, 20.0]),
            ],
        };
        let race = RaceXml {
            guid: "race-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            track_guid: Some("track-guid".into()),
            required_laps: Some(1),
            spawnpoint_id: None,
            passages: vec![
                RacePassage {
                    unique_id: "start".into(),
                    checkpoint_id: 1,
                    passage_type: "Start".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["left".into(), "right".into()],
                },
                RacePassage {
                    unique_id: "left".into(),
                    checkpoint_id: 2,
                    passage_type: "Pass".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["finish".into()],
                },
                RacePassage {
                    unique_id: "right".into(),
                    checkpoint_id: 3,
                    passage_type: "Pass".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["finish".into()],
                },
                RacePassage {
                    unique_id: "finish".into(),
                    checkpoint_id: 4,
                    passage_type: "Finish".into(),
                    directionality: "Any".into(),
                    next_ids: vec!["start".into()],
                },
            ],
        };
        let tracks = HashMap::from([(track.guid.clone(), track)]);
        let row = build_course("cache", &source, &race, &tracks)
            .unwrap()
            .unwrap();
        let course: ReplayCourseData = serde_json::from_str(&row.course_json).unwrap();

        assert_eq!(course.guide_path.unwrap().segments.len(), 4);
    }

    #[test]
    fn liftoff_and_micro_use_distinct_guide_curve_shapes() {
        let start = RacePassage {
            unique_id: "start".into(),
            checkpoint_id: 1,
            passage_type: "Start".into(),
            directionality: "Any".into(),
            next_ids: vec!["finish".into()],
        };
        let finish = RacePassage {
            unique_id: "finish".into(),
            checkpoint_id: 2,
            passage_type: "Finish".into(),
            directionality: "Any".into(),
            next_ids: vec![],
        };
        let start_bp = test_checkpoint(1, [0.0, 1.0, 0.0]);
        let finish_bp = test_checkpoint(2, [0.0, 1.0, 10.0]);
        let checkpoints = HashMap::from([(1, &start_bp), (2, &finish_bp)]);

        let liftoff = checkpoint_guide_segment(
            &start,
            &finish,
            Vec3::new(1.0, 0.0, 0.0),
            &checkpoints,
            GuideVariant::Liftoff,
        )
        .unwrap();
        let micro = checkpoint_guide_segment(
            &start,
            &finish,
            Vec3::new(1.0, 0.0, 0.0),
            &checkpoints,
            GuideVariant::MicroDrones,
        )
        .unwrap();

        assert_ne!(liftoff[liftoff.len() / 2], micro[micro.len() / 2]);
    }

    #[test]
    fn parses_guid_tags_and_str_tags() {
        let guid_xml = r#"<localID><guid>ABC</guid><type>TRACK</type></localID>"#;
        let str_xml = r#"<localID><str>DEF</str><type>TRACK</type></localID>"#;
        assert_eq!(local_id_guid(guid_xml).as_deref(), Some("ABC"));
        assert_eq!(local_id_guid(str_xml).as_deref(), Some("DEF"));

        let dependency_xml = r#"
<dependencies>
  <dependency><guid>track-guid</guid><type>TRACK</type></dependency>
  <dependency><str>race-guid</str><type>RACE</type></dependency>
</dependencies>"#;
        assert_eq!(
            first_dependency_guid(dependency_xml, "TRACK").as_deref(),
            Some("track-guid")
        );
        assert_eq!(
            first_dependency_guid(dependency_xml, "RACE").as_deref(),
            Some("race-guid")
        );
    }

    #[test]
    fn builds_checkpoints_from_flag_blueprints() {
        let source = AssetSource {
            game_title: "Liftoff".into(),
            label: "Test".into(),
            data_root: PathBuf::from("/tmp/liftoff-test"),
        };
        let track = TrackXml {
            guid: "track-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            environment_id: Some("TestEnv".into()),
            blueprints: vec![TrackBlueprint {
                kind: "TrackBlueprintFlag".into(),
                item_id: "AirgateBigLiftoffFinishWhite01".into(),
                instance_id: 7,
                position: Some([1.0, 2.0, 3.0]),
                rotation: Some([0.0, 45.0, 0.0]),
                dimensions: None,
                attach_points: Vec::new(),
                name: None,
            }],
        };
        let race = RaceXml {
            guid: "race-guid".into(),
            name: "01 - Test".into(),
            asset_key: None,
            track_guid: Some("track-guid".into()),
            required_laps: Some(1),
            spawnpoint_id: None,
            passages: vec![RacePassage {
                unique_id: "start".into(),
                checkpoint_id: 7,
                passage_type: "Start".into(),
                directionality: "Any".into(),
                next_ids: vec![],
            }],
        };
        let tracks = HashMap::from([(track.guid.clone(), track)]);
        let row = build_course("cache", &source, &race, &tracks)
            .unwrap()
            .unwrap();
        let course: ReplayCourseData = serde_json::from_str(&row.course_json).unwrap();
        assert_eq!(course.checkpoints.len(), 1);
        assert_eq!(course.checkpoints[0].checkpoint_id, 7);
        assert_eq!(
            course.checkpoints[0].item_id,
            "AirgateBigLiftoffFinishWhite01"
        );
    }

    #[test]
    #[ignore = "requires a local Liftoff Steam install"]
    fn extracts_field_day_from_installed_liftoff() {
        let source = discover_asset_sources()
            .into_iter()
            .find(|source| source.game_title == "Liftoff")
            .expect("local Liftoff install not found");
        let mut progress = |_progress: AssetRefreshProgress| {};
        let extracted = extract_install(&source, 0, 1, &mut progress).unwrap();
        let row = extracted
            .courses
            .iter()
            .find(|row| {
                row.race_name == "01 - Field Day"
                    && row.track_name.as_deref() == Some("01 - Field Day")
            })
            .expect("01 - Field Day course not extracted");
        let course: ReplayCourseData = serde_json::from_str(&row.course_json).unwrap();
        assert_eq!(course.race_guid, "fdca6e12-4dff-438d-91d8-bbabe74ae426");
        assert_eq!(
            course.track_guid.as_deref(),
            Some("add7945e-279a-42cb-8111-5ebc669292a5")
        );
        assert!(!course.checkpoints.is_empty());
    }

    #[test]
    fn extracts_quoted_vdf_fields() {
        let text = r#""name" "Liftoff Micro Drones"
"installdir" "Liftoff Micro Drones"
"path" "/tmp/SteamLibrary""#;
        assert_eq!(
            extract_vdf_value(text, "installdir").as_deref(),
            Some("Liftoff Micro Drones")
        );
        assert_eq!(extract_vdf_paths(text), vec!["/tmp/SteamLibrary"]);
    }
}
