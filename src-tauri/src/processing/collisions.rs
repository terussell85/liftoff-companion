//! Telemetry-only collision detection.
//!
//! Liftoff's UDP stream does not expose an explicit contact/collision flag, so
//! v1 infers impacts from abrupt speed loss plus evidence that movement was
//! actually impeded. A hard turn can briefly shed speed, but it usually keeps
//! making path progress and recovers quickly; those events are rejected.

use serde::{Deserialize, Serialize};

use crate::telemetry::sample::TelemetrySample;

#[derive(Debug, Clone)]
pub struct CollisionConfig {
    /// How far back to look for pre-impact speed.
    pub lookback_seconds: f64,
    /// How far forward to look for the lowest post-impact speed.
    pub recovery_seconds: f64,
    /// Events closer than this are treated as the same impact.
    pub merge_seconds: f64,
    /// Minimum pre-impact speed (m/s) required to consider an event.
    pub min_pre_speed: f32,
    /// Minimum speed loss (m/s) required to consider an event.
    pub min_speed_drop: f32,
    /// Minimum fractional speed loss unless the drone nearly stops.
    pub min_drop_ratio: f32,
    /// Speed (m/s) at or below which movement is considered stopped/impeded.
    pub stopped_speed: f32,
    /// Post-impact speed ratio at or below which movement is considered impeded.
    pub max_impeded_speed_ratio: f32,
    /// Minimum fractional speed loss for non-stop impeded events.
    pub min_impeded_drop_ratio: f32,
    /// Path distance / expected distance at or below which progress is impeded.
    pub max_impeded_path_ratio: f32,
    /// A fast recovery above this speed ratio is likely a turn, not a collision.
    pub turn_recovery_speed_ratio: f32,
    /// Path progress above this ratio supports classifying a speed dip as a turn.
    pub turn_path_ratio: f32,
    /// Pre-impact speed (m/s) that can contribute full speed score.
    pub full_impact_speed: f32,
    /// Deceleration (m/s^2) that can contribute full decel score.
    pub severe_decel_mps2: f32,
}

impl Default for CollisionConfig {
    fn default() -> Self {
        Self {
            lookback_seconds: 0.35,
            recovery_seconds: 0.45,
            merge_seconds: 0.5,
            min_pre_speed: 1.25,
            min_speed_drop: 0.75,
            min_drop_ratio: 0.25,
            stopped_speed: 0.5,
            max_impeded_speed_ratio: 0.45,
            min_impeded_drop_ratio: 0.4,
            max_impeded_path_ratio: 0.55,
            turn_recovery_speed_ratio: 0.75,
            turn_path_ratio: 0.65,
            full_impact_speed: 8.0,
            severe_decel_mps2: 25.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CollisionEvent {
    pub sample_index: usize,
    pub capture_time_seconds: f64,
    pub severity: u8,
    pub confidence: f32,
    pub speed_before: f32,
    pub speed_after: f32,
    pub speed_delta: f32,
    pub decel_mps2: f32,
    #[serde(default)]
    pub pos: Option<[f32; 3]>,
    #[serde(default)]
    pub geometry_confirmed: bool,
    #[serde(default)]
    pub geometry_status: Option<String>,
    #[serde(default)]
    pub hit_source: Option<String>,
    #[serde(default)]
    pub hit_label: Option<String>,
    #[serde(default)]
    pub hit_shape: Option<String>,
    #[serde(default)]
    pub hit_distance: Option<f32>,
}

#[derive(Debug, Clone)]
struct SpeedPoint {
    sample_index: usize,
    time: f64,
    speed: f32,
    pos: Option<[f32; 3]>,
}

pub fn detect_collisions(
    samples: &[TelemetrySample],
    cfg: &CollisionConfig,
) -> Vec<CollisionEvent> {
    let points: Vec<SpeedPoint> = samples
        .iter()
        .enumerate()
        .filter_map(|(sample_index, sample)| {
            let velocity = sample.velocity?;
            let speed = velocity.magnitude();
            if !sample.capture_time_seconds.is_finite() || !speed.is_finite() {
                return None;
            }
            Some(SpeedPoint {
                sample_index,
                time: sample.capture_time_seconds,
                speed,
                pos: sample.position.map(|p| [p.x, p.y, p.z]),
            })
        })
        .collect();

    let mut candidates = Vec::new();
    for i in 1..points.len() {
        let current = &points[i];
        let mut pre_index = 0usize;
        let mut pre_speed = f32::NEG_INFINITY;
        let mut pre_time = current.time;

        for j in (0..i).rev() {
            let prior = &points[j];
            if current.time - prior.time > cfg.lookback_seconds {
                break;
            }
            if prior.speed > pre_speed {
                pre_index = j;
                pre_speed = prior.speed;
                pre_time = prior.time;
            }
        }

        if !pre_speed.is_finite() || pre_speed < cfg.min_pre_speed {
            continue;
        }

        let mut post = current;
        let mut post_index = i;
        let mut recovery_index = i;
        for (j, point) in points.iter().enumerate().skip(i) {
            if point.time - current.time > cfg.recovery_seconds {
                break;
            }
            recovery_index = j;
            if point.speed < post.speed {
                post = point;
                post_index = j;
            }
        }

        let speed_delta = pre_speed - post.speed;
        if speed_delta < cfg.min_speed_drop {
            continue;
        }

        let drop_ratio = speed_delta / pre_speed.max(0.001);
        if drop_ratio < cfg.min_drop_ratio && post.speed > cfg.stopped_speed {
            continue;
        }

        let dt = (post.time - pre_time).max(0.001) as f32;
        let decel_mps2 = speed_delta / dt;
        let post_speed_ratio = post.speed / pre_speed.max(0.001);
        let recovery_speed_ratio = points[recovery_index].speed / pre_speed.max(0.001);
        let path_ratio = path_progress_ratio(&points, pre_index, recovery_index, pre_speed);
        let path_impeded = path_ratio
            .map(|ratio| ratio <= cfg.max_impeded_path_ratio)
            .unwrap_or(false);
        let stopped = post.speed <= cfg.stopped_speed;
        let speed_impeded = post_speed_ratio <= cfg.max_impeded_speed_ratio
            && drop_ratio >= cfg.min_impeded_drop_ratio;
        let turn_like_recovery = !stopped
            && recovery_speed_ratio >= cfg.turn_recovery_speed_ratio
            && path_ratio
                .map(|ratio| ratio >= cfg.turn_path_ratio)
                .unwrap_or(false);

        if turn_like_recovery {
            continue;
        }
        if !(stopped || speed_impeded || path_impeded) {
            continue;
        }

        let (severity, confidence) = score_collision(
            pre_speed,
            post.speed,
            speed_delta,
            decel_mps2,
            path_ratio,
            cfg,
        );

        candidates.push(CollisionEvent {
            sample_index: points[post_index].sample_index,
            capture_time_seconds: points[post_index].time,
            severity,
            confidence,
            speed_before: pre_speed,
            speed_after: points[post_index].speed,
            speed_delta,
            decel_mps2,
            pos: points[post_index].pos,
            geometry_confirmed: false,
            geometry_status: Some("not_checked".into()),
            hit_source: None,
            hit_label: None,
            hit_shape: None,
            hit_distance: None,
        });
    }

    merge_candidates(candidates, cfg.merge_seconds)
}

fn score_collision(
    speed_before: f32,
    speed_after: f32,
    speed_delta: f32,
    decel_mps2: f32,
    path_ratio: Option<f32>,
    cfg: &CollisionConfig,
) -> (u8, f32) {
    let drop_score = (speed_delta / speed_before.max(0.001)).clamp(0.0, 1.0);
    let speed_score = (speed_before / cfg.full_impact_speed.max(0.001)).clamp(0.0, 1.0);
    let impeded_score = if speed_after <= cfg.stopped_speed {
        1.0
    } else {
        (1.0 - speed_after / (speed_before * 0.75).max(0.001)).clamp(0.0, 1.0)
    };
    let decel_score = (decel_mps2 / cfg.severe_decel_mps2.max(0.001)).clamp(0.0, 1.0);
    let path_score = path_ratio
        .map(|ratio| (1.0 - ratio / cfg.max_impeded_path_ratio.max(0.001)).clamp(0.0, 1.0))
        .unwrap_or(0.0);

    let raw = 10.0
        * (0.3 * drop_score
            + 0.22 * speed_score
            + 0.25 * impeded_score
            + 0.13 * decel_score
            + 0.1 * path_score);
    let severity = raw.round().clamp(1.0, 10.0) as u8;
    let confidence =
        (0.25 + 0.2 * drop_score + 0.25 * impeded_score + 0.15 * decel_score + 0.15 * path_score)
            .clamp(0.0, 1.0);

    (severity, confidence)
}

fn path_progress_ratio(
    points: &[SpeedPoint],
    start_index: usize,
    end_index: usize,
    expected_speed: f32,
) -> Option<f32> {
    if end_index <= start_index {
        return None;
    }
    let expected_distance =
        expected_speed * (points[end_index].time - points[start_index].time).max(0.001) as f32;
    if expected_distance <= 0.001 {
        return None;
    }

    let mut actual_distance = 0.0f32;
    for pair in points[start_index..=end_index].windows(2) {
        let a = pair[0].pos?;
        let b = pair[1].pos?;
        actual_distance += distance(a, b);
    }

    Some((actual_distance / expected_distance).clamp(0.0, 2.0))
}

fn distance(a: [f32; 3], b: [f32; 3]) -> f32 {
    let dx = b[0] - a[0];
    let dy = b[1] - a[1];
    let dz = b[2] - a[2];
    (dx * dx + dy * dy + dz * dz).sqrt()
}

fn merge_candidates(candidates: Vec<CollisionEvent>, merge_seconds: f64) -> Vec<CollisionEvent> {
    let mut merged: Vec<CollisionEvent> = Vec::new();
    for candidate in candidates {
        if let Some(last) = merged.last_mut() {
            if candidate.capture_time_seconds - last.capture_time_seconds <= merge_seconds {
                if candidate.severity > last.severity
                    || (candidate.severity == last.severity
                        && candidate.speed_delta > last.speed_delta)
                {
                    *last = candidate;
                }
                continue;
            }
        }
        merged.push(candidate);
    }
    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::sample::{TelemetrySample, Vec3};

    fn sample(t: f64, speed: f32) -> TelemetrySample {
        sample_at(t, [t as f32, 1.0, 0.0], [speed, 0.0, 0.0])
    }

    fn sample_at(t: f64, position: [f32; 3], velocity: [f32; 3]) -> TelemetrySample {
        TelemetrySample {
            capture_time_seconds: t,
            position: Some(Vec3 {
                x: position[0],
                y: position[1],
                z: position[2],
            }),
            velocity: Some(Vec3 {
                x: velocity[0],
                y: velocity[1],
                z: velocity[2],
            }),
            ..Default::default()
        }
    }

    #[test]
    fn steady_speed_produces_no_events() {
        let samples: Vec<_> = (0..40).map(|i| sample(i as f64 * 0.1, 4.0)).collect();
        let events = detect_collisions(&samples, &CollisionConfig::default());
        assert!(events.is_empty());
    }

    #[test]
    fn high_speed_full_stop_scores_as_full_impact() {
        let mut samples = Vec::new();
        for i in 0..10 {
            samples.push(sample(i as f64 * 0.1, 10.0));
        }
        for i in 10..25 {
            samples.push(sample(i as f64 * 0.1, 0.0));
        }

        let events = detect_collisions(&samples, &CollisionConfig::default());
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].severity, 10);
        assert!(events[0].confidence > 0.9);
    }

    #[test]
    fn moderate_glancing_drop_scores_lower() {
        let mut samples = Vec::new();
        for i in 0..10 {
            let t = i as f64 * 0.1;
            samples.push(sample_at(t, [t as f32 * 4.0, 1.0, 0.0], [4.0, 0.0, 0.0]));
        }
        for i in 10..20 {
            let t = i as f64 * 0.1;
            samples.push(sample_at(
                t,
                [3.6 + (i - 9) as f32 * 0.05, 1.0, 0.0],
                [1.5, 0.0, 0.0],
            ));
        }

        let events = detect_collisions(&samples, &CollisionConfig::default());
        assert_eq!(events.len(), 1);
        assert!((4..=7).contains(&events[0].severity), "{:?}", events[0]);
    }

    #[test]
    fn hard_direction_change_with_recovery_is_not_collision() {
        let mut samples = Vec::new();
        for i in 0..12 {
            let t = i as f64 * 0.1;
            let speed = if i == 5 { 3.0 } else { 6.0 };
            let position = if i <= 5 {
                [i as f32 * 0.6, 1.0, 0.0]
            } else {
                [3.0, 1.0, (i - 5) as f32 * 0.6]
            };
            let velocity = if i <= 5 {
                [speed, 0.0, 0.0]
            } else {
                [0.0, 0.0, speed]
            };
            samples.push(sample_at(t, position, velocity));
        }

        let events = detect_collisions(&samples, &CollisionConfig::default());
        assert!(events.is_empty(), "{events:?}");
    }

    #[test]
    fn closely_spaced_candidates_merge() {
        let samples = vec![
            sample(0.0, 8.0),
            sample(0.1, 8.0),
            sample(0.2, 1.0),
            sample(0.3, 7.0),
            sample(0.4, 0.0),
            sample(1.2, 6.0),
            sample(1.3, 0.0),
        ];

        let events = detect_collisions(&samples, &CollisionConfig::default());
        assert_eq!(events.len(), 2);
        assert_eq!(events[0].severity, 10);
    }
}
