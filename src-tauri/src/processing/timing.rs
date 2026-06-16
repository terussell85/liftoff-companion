use std::collections::HashMap;

use crate::liftoff::assets::{ReplayCheckpoint, ReplayCourseData};
use crate::storage::repositories::{
    RaceGateSplitRow, RaceLapRow, RacePassageEventRow, RaceSessionRow,
};
use crate::telemetry::sample::TelemetrySample;

const GATE_MARGIN_METERS: f32 = 0.30;
const MIN_GATE_EXTENT_METERS: f32 = 0.04;
const PASSAGE_DEBOUNCE_SECONDS: f64 = 0.35;
const SESSION_END_GRACE_SECONDS: f64 = 1.0;
const EPSILON: f32 = 1.0e-5;

#[derive(Debug, Default)]
pub struct TimingRows {
    pub laps: Vec<RaceLapRow>,
    pub gate_splits: Vec<RaceGateSplitRow>,
    pub passage_events: Vec<RacePassageEventRow>,
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

    fn sub(self, other: Self) -> Self {
        Self::new(self.x - other.x, self.y - other.y, self.z - other.z)
    }

    fn distance_squared(self, other: Self) -> f32 {
        let delta = self.sub(other);
        delta.x * delta.x + delta.y * delta.y + delta.z * delta.z
    }

    fn axis(self, axis: usize) -> f32 {
        match axis {
            0 => self.x,
            1 => self.y,
            _ => self.z,
        }
    }
}

#[derive(Debug, Clone)]
struct TimedPoint {
    sample_index: i64,
    time_seconds: f64,
    position: Vec3,
}

#[derive(Debug, Clone)]
struct PreparedGate {
    checkpoint_id: i64,
    sequence_index: i64,
    passage_type: String,
    directionality: String,
    center: Vec3,
    rotation: [f32; 3],
    half_extents: Vec3,
    normal_axis: usize,
}

#[derive(Debug, Clone)]
struct PassageHit {
    gate_index: usize,
    checkpoint_id: i64,
    sequence_index: i64,
    passage_type: String,
    directionality: String,
    time_seconds: f64,
    sample_index: Option<i64>,
    confidence: f64,
}

pub fn derive_session_timing(
    dataset_id: &str,
    capture_id: &str,
    session: &RaceSessionRow,
    course: &ReplayCourseData,
    samples: &[TelemetrySample],
    offset_seconds: f64,
    total_seconds: f64,
) -> TimingRows {
    let session_start = session.start_seconds;
    let session_end = session.end_seconds.unwrap_or(total_seconds);
    if session_end <= session_start || course.checkpoints.len() < 2 {
        return TimingRows::default();
    }

    let mut gates = course
        .checkpoints
        .iter()
        .map(|checkpoint| PreparedGate::new(checkpoint, &course.game_title))
        .collect::<Vec<_>>();
    let finish_index = gates
        .iter()
        .rposition(|gate| gate.passage_type.eq_ignore_ascii_case("finish"))
        .unwrap_or_else(|| gates.len().saturating_sub(1));
    gates.truncate(finish_index + 1);
    let duplicate_terminal_finish = gates.last().is_some_and(|last| {
        last.passage_type.eq_ignore_ascii_case("finish") && same_physical_gate(&gates[0], last)
    });
    if duplicate_terminal_finish {
        gates.pop();
    }
    if gates.len() < 2 {
        return TimingRows::default();
    }
    let distinct_terminal_finish = gates.last().is_some_and(|last| {
        last.passage_type.eq_ignore_ascii_case("finish") && !same_physical_gate(&gates[0], last)
    });
    let finish_gate_index = distinct_terminal_finish.then_some(gates.len() - 1);
    let required_laps = course.required_laps.filter(|laps| *laps > 0);
    let detection_end = if required_laps.is_some() {
        session_end + SESSION_END_GRACE_SECONDS
    } else {
        session_end
    };

    let points = samples
        .iter()
        .enumerate()
        .filter_map(|(index, sample)| {
            let position = sample.position?;
            let time_seconds = sample.capture_time_seconds + offset_seconds;
            if time_seconds < session_start || time_seconds > detection_end {
                return None;
            }
            Some(TimedPoint {
                sample_index: index as i64,
                time_seconds,
                position: Vec3::new(position.x, position.y, position.z),
            })
        })
        .collect::<Vec<_>>();
    if points.is_empty() {
        return TimingRows::default();
    }

    let mut rows = TimingRows::default();
    let mut expected_gate = 0usize;
    let mut attempt_index = 1i64;
    let mut lap_start: Option<PassageHit> = None;
    let mut previous_lap_hit: Option<PassageHit> = None;
    let mut lap_confidences = Vec::<f64>::new();
    let mut last_by_sequence = HashMap::<i64, f64>::new();
    let mut previous_point: Option<&TimedPoint> = None;

    for point in &points {
        if point.time_seconds > session_end && !(expected_gate == 0 && lap_start.is_some()) {
            break;
        }

        if let Some(previous_hit) = previous_lap_hit.as_ref() {
            let gate = &gates[previous_hit.gate_index];
            if previous_hit.gate_index != expected_gate {
                if let Some(hit) =
                    detect_passage(previous_point, point, gate, previous_hit.gate_index)
                {
                    if !is_debounced(&last_by_sequence, &hit) {
                        last_by_sequence.insert(hit.sequence_index, hit.time_seconds);
                        rows.passage_events.push(passage_event_row(
                            dataset_id,
                            capture_id,
                            &session.id,
                            attempt_index,
                            &hit,
                        ));
                        update_last_split_to(&mut rows.gate_splits, attempt_index, &hit);
                        if hit.gate_index == 0 {
                            lap_start = Some(hit.clone());
                        }
                        previous_lap_hit = Some(hit);
                    }
                    previous_point = Some(point);
                    continue;
                }
            }
        }

        let gate = &gates[expected_gate];
        let Some(hit) = detect_passage(previous_point, point, gate, expected_gate) else {
            previous_point = Some(point);
            continue;
        };
        if is_debounced(&last_by_sequence, &hit) {
            previous_point = Some(point);
            continue;
        }
        last_by_sequence.insert(hit.sequence_index, hit.time_seconds);

        rows.passage_events.push(passage_event_row(
            dataset_id,
            capture_id,
            &session.id,
            attempt_index,
            &hit,
        ));

        if expected_gate == 0 && lap_start.is_some() && !distinct_terminal_finish {
            if let Some(previous) = previous_lap_hit.as_ref() {
                rows.gate_splits.push(gate_split_row(
                    dataset_id,
                    capture_id,
                    &session.id,
                    attempt_index,
                    previous.gate_index as i64,
                    "lap_section",
                    previous,
                    &hit,
                ));
            }
            lap_confidences.push(hit.confidence);
            if let Some(start) = lap_start.as_ref() {
                rows.laps.push(lap_row(
                    dataset_id,
                    capture_id,
                    &session.id,
                    attempt_index,
                    start,
                    &hit,
                    mean_confidence(&lap_confidences),
                ));
            }
            if required_laps.is_some_and(|required| attempt_index >= required) {
                break;
            }
            attempt_index += 1;
            lap_start = Some(hit.clone());
            previous_lap_hit = Some(hit.clone());
            lap_confidences.clear();
            lap_confidences.push(hit.confidence);
        } else if expected_gate == 0 {
            lap_start = Some(hit.clone());
            previous_lap_hit = Some(hit.clone());
            lap_confidences.clear();
            lap_confidences.push(hit.confidence);
        } else {
            if let Some(previous) = previous_lap_hit.as_ref() {
                rows.gate_splits.push(gate_split_row(
                    dataset_id,
                    capture_id,
                    &session.id,
                    attempt_index,
                    previous.gate_index as i64,
                    "lap_section",
                    previous,
                    &hit,
                ));
            }
            lap_confidences.push(hit.confidence);
            previous_lap_hit = Some(hit.clone());
            if finish_gate_index == Some(expected_gate) {
                if let Some(start) = lap_start.as_ref() {
                    rows.laps.push(lap_row(
                        dataset_id,
                        capture_id,
                        &session.id,
                        attempt_index,
                        start,
                        &hit,
                        mean_confidence(&lap_confidences),
                    ));
                }
                if required_laps.is_some_and(|required| attempt_index >= required) {
                    break;
                }
                attempt_index += 1;
                lap_start = None;
                previous_lap_hit = None;
                lap_confidences.clear();
            }
        }

        expected_gate = (expected_gate + 1) % gates.len();
        previous_point = Some(point);
    }

    rows
}

impl PreparedGate {
    fn new(checkpoint: &ReplayCheckpoint, game_title: &str) -> Self {
        let dimensions = gate_dimensions(checkpoint.dimensions, game_title);
        let normal_axis = smallest_axis(dimensions);
        Self {
            checkpoint_id: checkpoint.checkpoint_id,
            sequence_index: checkpoint.sequence_index,
            passage_type: checkpoint.passage_type.clone(),
            directionality: checkpoint.directionality.clone(),
            center: Vec3::from_array(checkpoint.position),
            rotation: checkpoint.rotation,
            half_extents: Vec3::new(
                (dimensions[0].abs() * 0.5).max(MIN_GATE_EXTENT_METERS) + GATE_MARGIN_METERS,
                (dimensions[1].abs() * 0.5).max(MIN_GATE_EXTENT_METERS) + GATE_MARGIN_METERS,
                (dimensions[2].abs() * 0.5).max(MIN_GATE_EXTENT_METERS) + GATE_MARGIN_METERS,
            ),
            normal_axis,
        }
    }

    fn to_local(&self, point: Vec3) -> Vec3 {
        inverse_rotate_euler_zxy(point.sub(self.center), self.rotation)
    }

    fn contains_local(&self, local: Vec3) -> bool {
        local.x.abs() <= self.half_extents.x
            && local.y.abs() <= self.half_extents.y
            && local.z.abs() <= self.half_extents.z
    }
}

fn same_physical_gate(first: &PreparedGate, last: &PreparedGate) -> bool {
    first.checkpoint_id == last.checkpoint_id
        || first.center.distance_squared(last.center) < EPSILON
}

fn detect_passage(
    previous: Option<&TimedPoint>,
    current: &TimedPoint,
    gate: &PreparedGate,
    gate_index: usize,
) -> Option<PassageHit> {
    match previous {
        Some(previous) => {
            let prev_local = gate.to_local(previous.position);
            if gate.contains_local(prev_local) {
                return None;
            }
            let current_local = gate.to_local(current.position);
            let fraction =
                segment_box_entry_fraction(prev_local, current_local, gate.half_extents)?;
            let dt = current.time_seconds - previous.time_seconds;
            if dt < 0.0 {
                return None;
            }
            let time_seconds = previous.time_seconds + dt * fraction as f64;
            Some(PassageHit {
                gate_index,
                checkpoint_id: gate.checkpoint_id,
                sequence_index: gate.sequence_index,
                passage_type: gate.passage_type.clone(),
                directionality: gate.directionality.clone(),
                time_seconds,
                sample_index: Some(current.sample_index),
                confidence: passage_confidence(
                    gate,
                    current_local.sub(prev_local),
                    fraction > EPSILON && fraction < 1.0 - EPSILON,
                ),
            })
        }
        None => {
            let current_local = gate.to_local(current.position);
            if !gate.contains_local(current_local) {
                return None;
            }
            Some(PassageHit {
                gate_index,
                checkpoint_id: gate.checkpoint_id,
                sequence_index: gate.sequence_index,
                passage_type: gate.passage_type.clone(),
                directionality: gate.directionality.clone(),
                time_seconds: current.time_seconds,
                sample_index: Some(current.sample_index),
                confidence: 0.78,
            })
        }
    }
}

fn is_debounced(last_by_sequence: &HashMap<i64, f64>, hit: &PassageHit) -> bool {
    last_by_sequence
        .get(&hit.sequence_index)
        .is_some_and(|last| hit.time_seconds - *last < PASSAGE_DEBOUNCE_SECONDS)
}

fn update_last_split_to(
    gate_splits: &mut [RaceGateSplitRow],
    attempt_index: i64,
    hit: &PassageHit,
) {
    let Some(split) = gate_splits.iter_mut().rev().find(|split| {
        split.lap_index == attempt_index
            && split.to_checkpoint_sequence == Some(hit.sequence_index)
            && split.end_seconds <= hit.time_seconds
    }) else {
        return;
    };
    split.end_seconds = hit.time_seconds;
    split.duration_seconds = hit.time_seconds - split.start_seconds;
    split.end_sample_index = hit.sample_index;
    split.confidence = ((split.confidence + hit.confidence) * 0.5).clamp(0.0, 1.0);
}

fn segment_box_entry_fraction(start: Vec3, end: Vec3, extents: Vec3) -> Option<f32> {
    let delta = end.sub(start);
    let mut t_min = 0.0f32;
    let mut t_max = 1.0f32;

    for axis in 0..3 {
        let p = start.axis(axis);
        let d = delta.axis(axis);
        let e = extents.axis(axis);
        if d.abs() <= EPSILON {
            if p < -e || p > e {
                return None;
            }
            continue;
        }
        let inv = 1.0 / d;
        let mut t1 = (-e - p) * inv;
        let mut t2 = (e - p) * inv;
        if t1 > t2 {
            std::mem::swap(&mut t1, &mut t2);
        }
        t_min = t_min.max(t1);
        t_max = t_max.min(t2);
        if t_min > t_max {
            return None;
        }
    }

    if !(0.0..=1.0).contains(&t_min) {
        return None;
    }
    Some(t_min)
}

fn passage_confidence(gate: &PreparedGate, local_delta: Vec3, interpolated: bool) -> f64 {
    let mut confidence: f64 = if interpolated { 0.90 } else { 0.84 };
    let dir = gate.directionality.to_ascii_lowercase();
    if dir != "any" {
        let travel = local_delta.axis(gate.normal_axis);
        if travel.abs() > EPSILON {
            let expected = if dir == "righttoleft" { -1.0 } else { 1.0 };
            confidence += if travel.signum() == expected {
                0.06
            } else {
                -0.14
            };
        }
    }
    confidence.clamp(0.55, 0.98)
}

fn lap_row(
    dataset_id: &str,
    capture_id: &str,
    session_id: &str,
    lap_index: i64,
    start: &PassageHit,
    finish: &PassageHit,
    confidence: f64,
) -> RaceLapRow {
    RaceLapRow {
        id: format!("lap_{}", uuid::Uuid::new_v4().simple()),
        dataset_id: dataset_id.to_string(),
        capture_id: capture_id.to_string(),
        session_id: session_id.to_string(),
        lap_index,
        start_seconds: start.time_seconds,
        end_seconds: finish.time_seconds,
        duration_seconds: finish.time_seconds - start.time_seconds,
        start_sample_index: start.sample_index,
        end_sample_index: finish.sample_index,
        status: "completed".into(),
        confidence,
    }
}

#[allow(clippy::too_many_arguments)]
fn gate_split_row(
    dataset_id: &str,
    capture_id: &str,
    session_id: &str,
    lap_index: i64,
    section_index: i64,
    section_kind: &str,
    from: &PassageHit,
    to: &PassageHit,
) -> RaceGateSplitRow {
    RaceGateSplitRow {
        id: format!("split_{}", uuid::Uuid::new_v4().simple()),
        dataset_id: dataset_id.to_string(),
        capture_id: capture_id.to_string(),
        session_id: session_id.to_string(),
        lap_index,
        section_index,
        section_kind: section_kind.to_string(),
        from_checkpoint_id: Some(from.checkpoint_id),
        from_checkpoint_sequence: Some(from.sequence_index),
        from_passage_type: Some(from.passage_type.clone()),
        to_checkpoint_id: Some(to.checkpoint_id),
        to_checkpoint_sequence: Some(to.sequence_index),
        to_passage_type: Some(to.passage_type.clone()),
        start_seconds: from.time_seconds,
        end_seconds: to.time_seconds,
        duration_seconds: to.time_seconds - from.time_seconds,
        start_sample_index: from.sample_index,
        end_sample_index: to.sample_index,
        confidence: ((from.confidence + to.confidence) * 0.5).clamp(0.0, 1.0),
    }
}

fn passage_event_row(
    dataset_id: &str,
    capture_id: &str,
    session_id: &str,
    lap_index: i64,
    hit: &PassageHit,
) -> RacePassageEventRow {
    RacePassageEventRow {
        id: format!("pass_{}", uuid::Uuid::new_v4().simple()),
        dataset_id: dataset_id.to_string(),
        capture_id: capture_id.to_string(),
        session_id: session_id.to_string(),
        lap_index,
        checkpoint_id: hit.checkpoint_id,
        checkpoint_sequence: hit.sequence_index,
        passage_type: hit.passage_type.clone(),
        directionality: hit.directionality.clone(),
        event_seconds: hit.time_seconds,
        sample_index: hit.sample_index,
        confidence: hit.confidence,
    }
}

fn mean_confidence(values: &[f64]) -> f64 {
    if values.is_empty() {
        return 0.0;
    }
    values.iter().sum::<f64>() / values.len() as f64
}

fn gate_dimensions(dimensions: [f32; 3], game_title: &str) -> [f32; 3] {
    if is_micro_drones(game_title) {
        return dimensions.map(|v| v.abs());
    }
    [
        dimensions[2].abs(),
        dimensions[1].abs(),
        dimensions[0].abs(),
    ]
}

fn is_micro_drones(game_title: &str) -> bool {
    game_title
        .to_ascii_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .contains("microdrones")
}

fn smallest_axis(dimensions: [f32; 3]) -> usize {
    if dimensions[0] <= dimensions[1] && dimensions[0] <= dimensions[2] {
        0
    } else if dimensions[1] <= dimensions[0] && dimensions[1] <= dimensions[2] {
        1
    } else {
        2
    }
}

fn inverse_rotate_euler_zxy(value: Vec3, degrees: [f32; 3]) -> Vec3 {
    let [x, y, z] = degrees.map(f32::to_radians);
    rotate_z(rotate_x(rotate_y(value, -y), -x), -z)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::liftoff::assets::ReplayCourseData;
    use crate::storage::repositories::RaceSessionRow;
    use crate::telemetry::sample::{TelemetrySample, Vec3 as SampleVec3};

    fn checkpoint(sequence: i64, kind: &str, x: f32) -> ReplayCheckpoint {
        checkpoint_with_id(sequence, sequence, kind, x)
    }

    fn checkpoint_with_id(
        sequence: i64,
        checkpoint_id: i64,
        kind: &str,
        x: f32,
    ) -> ReplayCheckpoint {
        ReplayCheckpoint {
            sequence_index: sequence,
            checkpoint_id,
            passage_type: kind.into(),
            directionality: "LeftToRight".into(),
            item_id: "CheckpointBoxFlexible01".into(),
            position: [x, 0.0, 0.0],
            rotation: [0.0, 0.0, 0.0],
            dimensions: [0.2, 4.0, 4.0],
        }
    }

    fn course() -> ReplayCourseData {
        ReplayCourseData {
            cache_id: "cache".into(),
            game_title: "Liftoff Micro Drones".into(),
            data_root: "/tmp".into(),
            race_guid: "race".into(),
            race_name: "Race".into(),
            race_asset_key: None,
            track_guid: None,
            track_name: None,
            track_asset_key: None,
            environment_id: None,
            required_laps: Some(2),
            checkpoints: vec![
                checkpoint(0, "Start", 0.0),
                checkpoint(1, "Pass", 10.0),
                checkpoint(2, "Finish", 20.0),
            ],
            spawnpoint: None,
            props: Vec::new(),
            collision_props: Vec::new(),
            guide_path: None,
        }
    }

    fn course_with_shared_start_finish() -> ReplayCourseData {
        ReplayCourseData {
            cache_id: "cache".into(),
            game_title: "Liftoff Micro Drones".into(),
            data_root: "/tmp".into(),
            race_guid: "race".into(),
            race_name: "Race".into(),
            race_asset_key: None,
            track_guid: None,
            track_name: None,
            track_asset_key: None,
            environment_id: None,
            required_laps: Some(3),
            checkpoints: vec![
                checkpoint_with_id(0, 100, "Start", 0.0),
                checkpoint_with_id(1, 200, "Pass", 10.0),
                checkpoint_with_id(2, 100, "Finish", 0.0),
            ],
            spawnpoint: None,
            props: Vec::new(),
            collision_props: Vec::new(),
            guide_path: None,
        }
    }

    fn session(end: f64) -> RaceSessionRow {
        RaceSessionRow {
            id: "rs".into(),
            capture_id: "cap".into(),
            session_index: 0,
            start_monotonic_ns: 0,
            end_monotonic_ns: Some((end * 1_000_000_000.0) as i64),
            start_seconds: 0.0,
            end_seconds: Some(end),
            duration_seconds: Some(end),
            level: None,
            race: Some("Race".into()),
            track: None,
            game_mode: Some("Race".into()),
            drone: None,
            race_guid: Some("race".into()),
            title: None,
            segmentation_method: "test".into(),
            confidence: Some(1.0),
            collision_count: 0,
            collision_max_severity: 0,
            collision_avg_severity: None,
        }
    }

    fn sample(t: f64, x: f32) -> TelemetrySample {
        TelemetrySample {
            capture_time_seconds: t,
            position: Some(SampleVec3 { x, y: 0.0, z: 0.0 }),
            ..Default::default()
        }
    }

    #[test]
    fn detects_completed_laps_and_gate_sections_for_distinct_finish() {
        let samples = vec![
            sample(0.0, -2.0),
            sample(1.0, 1.0),
            sample(5.0, 9.0),
            sample(6.0, 11.0),
            sample(9.0, 19.0),
            sample(10.0, 21.0),
            sample(12.0, -2.0),
            sample(13.0, 1.0),
            sample(16.0, 9.0),
            sample(17.0, 11.0),
            sample(20.0, 19.0),
            sample(21.0, 21.0),
        ];

        let rows =
            derive_session_timing("ds", "cap", &session(30.0), &course(), &samples, 0.0, 30.0);

        assert_eq!(rows.laps.len(), 2);
        assert_eq!(rows.laps[0].lap_index, 1);
        assert!((rows.laps[0].duration_seconds - 8.8).abs() < 0.2);
        assert!((rows.laps[1].end_seconds - 20.3).abs() < 0.2);
        assert_eq!(
            rows.gate_splits
                .iter()
                .filter(|split| split.section_kind == "lap_section")
                .count(),
            4
        );
        assert!(
            !rows
                .gate_splits
                .iter()
                .any(|split| split.section_kind == "connector")
        );
        assert_eq!(
            rows.gate_splits[1].to_passage_type.as_deref(),
            Some("Finish")
        );
        assert_contiguous_lap_sections(&rows.gate_splits, 1, 2);
        assert_contiguous_lap_sections(&rows.gate_splits, 2, 2);
    }

    #[test]
    fn collapses_shared_start_finish_and_counts_final_lap_at_session_end() {
        let samples = vec![
            sample(0.0, -2.0),
            sample(1.0, 1.0),
            sample(4.0, 9.0),
            sample(5.0, 11.0),
            sample(9.0, 2.0),
            sample(10.0, -1.0),
            sample(14.0, 9.0),
            sample(15.0, 11.0),
            sample(19.0, 2.0),
            sample(20.0, -1.0),
            sample(24.0, 9.0),
            sample(25.0, 11.0),
            sample(29.0, 2.0),
            sample(30.0, -1.0),
        ];

        let rows = derive_session_timing(
            "ds",
            "cap",
            &session(29.4),
            &course_with_shared_start_finish(),
            &samples,
            0.0,
            30.0,
        );

        assert_eq!(rows.laps.len(), 3);
        assert_eq!(rows.laps[2].lap_index, 3);
        assert_eq!(rows.gate_splits.len(), 6);
        assert_eq!(
            rows.gate_splits
                .iter()
                .filter(|split| split.lap_index == 3)
                .count(),
            2
        );
        assert!(rows.gate_splits.iter().all(|split| {
            split.from_checkpoint_id.is_some() && split.from_checkpoint_id != split.to_checkpoint_id
        }));
    }

    #[test]
    fn ignores_out_of_order_gate_crossings() {
        let mut course = course();
        course.checkpoints[1].position = [10.0, 10.0, 0.0];
        let samples = vec![
            sample(2.0, -2.0),
            sample(3.0, 1.0),
            sample(4.0, 19.0),
            sample(5.0, 21.0),
        ];

        let rows = derive_session_timing("ds", "cap", &session(10.0), &course, &samples, 0.0, 10.0);

        assert!(rows.laps.is_empty());
        assert_eq!(rows.passage_events.len(), 1);
        assert_eq!(rows.passage_events[0].passage_type, "Start");
    }

    #[test]
    fn keeps_partial_gate_splits_without_completed_lap() {
        let samples = vec![
            sample(0.0, -2.0),
            sample(1.0, 1.0),
            sample(4.0, 9.0),
            sample(5.0, 11.0),
        ];

        let rows =
            derive_session_timing("ds", "cap", &session(10.0), &course(), &samples, 0.0, 10.0);

        assert!(rows.laps.is_empty());
        assert_eq!(rows.gate_splits.len(), 1);
        assert_eq!(rows.gate_splits[0].section_kind, "lap_section");
    }

    #[test]
    fn repeated_same_gate_uses_last_hit_for_next_section() {
        let samples = vec![
            sample(0.0, -2.0),
            sample(1.0, 1.0),
            sample(2.0, -2.0),
            sample(3.0, 1.0),
            sample(4.0, 9.0),
            sample(5.0, 11.0),
        ];

        let rows =
            derive_session_timing("ds", "cap", &session(10.0), &course(), &samples, 0.0, 10.0);

        assert!(rows.laps.is_empty());
        assert_eq!(rows.gate_splits.len(), 1);
        assert_eq!(
            rows.gate_splits[0].from_passage_type.as_deref(),
            Some("Start")
        );
        assert_eq!(rows.gate_splits[0].to_passage_type.as_deref(), Some("Pass"));
        assert!(
            rows.gate_splits[0].start_seconds > 2.0,
            "same-gate repeat should move section start forward: {:?}",
            rows.gate_splits[0]
        );
        assert!(rows.gate_splits[0].duration_seconds < 2.0);
    }

    #[test]
    fn segment_intersection_detects_fast_pass_through_gate() {
        let samples = vec![sample(0.0, -2.0), sample(1.0, 22.0)];

        let rows = derive_session_timing("ds", "cap", &session(2.0), &course(), &samples, 0.0, 2.0);

        assert_eq!(rows.passage_events.len(), 1);
        assert_eq!(rows.passage_events[0].passage_type, "Start");
        assert!(rows.passage_events[0].event_seconds > 0.0);
    }

    fn assert_contiguous_lap_sections(
        splits: &[RaceGateSplitRow],
        lap_index: i64,
        expected_count: usize,
    ) {
        let lap_splits = splits
            .iter()
            .filter(|split| split.lap_index == lap_index)
            .collect::<Vec<_>>();
        assert_eq!(lap_splits.len(), expected_count);
        for pair in lap_splits.windows(2) {
            assert!(
                (pair[0].end_seconds - pair[1].start_seconds).abs() < 1e-9,
                "gate sections should not have timing gaps: {:?} then {:?}",
                pair[0],
                pair[1]
            );
        }
    }
}
