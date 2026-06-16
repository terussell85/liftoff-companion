use std::collections::HashMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::capture::integrity::verify_file_hash;
use crate::capture::rawcap::RawCapReader;
use crate::error::AppResult;
use crate::processing::collisions::{detect_collisions, CollisionConfig, CollisionEvent};
use crate::telemetry::liftoff_schema::LiftoffSchema;
use crate::telemetry::parser::{parse_packet, ParserWarning};
use crate::telemetry::sample::TelemetrySample;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineSummary {
    pub packet_count: u64,
    pub sample_count: u64,
    pub warning_count: u64,
    pub warnings_by_kind: HashMap<String, u64>,
    pub schema_endpoint: String,
    pub schema_field_count: usize,
    pub schema_config_hash: String,
    pub mean_speed: f32,
    pub max_speed: f32,
    pub min_speed: f32,
    #[serde(default)]
    pub collision_count: u64,
    #[serde(default)]
    pub collision_max_severity: u8,
    #[serde(default)]
    pub collision_avg_severity: Option<f32>,
    /// First packet's capture-start-relative monotonic time (ns). Sample
    /// `capture_time_seconds` is relative to this; add it back to align with
    /// capture-start-relative markers and race-session windows.
    pub start_monotonic_ns: u64,
}

pub struct PipelineOutput {
    pub summary: PipelineSummary,
    pub samples: Vec<TelemetrySample>,
    pub collision_events: Vec<CollisionEvent>,
}

pub fn run_pipeline<F>(
    rawcap_path: &Path,
    expected_hash: Option<&str>,
    schema: &LiftoffSchema,
    mut progress_cb: F,
) -> AppResult<PipelineOutput>
where
    F: FnMut(u64),
{
    if let Some(expected) = expected_hash {
        verify_file_hash(rawcap_path, expected)?;
    }

    let mut reader = RawCapReader::open(rawcap_path)?;
    let mut samples = Vec::new();
    let mut warning_count = 0u64;
    let mut warnings_by_kind: HashMap<String, u64> = HashMap::new();
    let mut packet_count = 0u64;

    let mut min_speed = f32::INFINITY;
    let mut max_speed = f32::NEG_INFINITY;
    let mut speed_sum: f64 = 0.0;
    let mut speed_samples: u64 = 0;

    let mut start_monotonic: Option<u64> = None;

    while let Some(rec) = reader.next_record()? {
        let monotonic_origin = *start_monotonic.get_or_insert(rec.monotonic_ns);
        let capture_time_seconds = if rec.monotonic_ns >= monotonic_origin {
            (rec.monotonic_ns - monotonic_origin) as f64 / 1_000_000_000.0
        } else {
            0.0
        };
        let (sample, warnings) = parse_packet(
            &rec.payload,
            schema,
            rec.sequence_number,
            capture_time_seconds,
        );
        if let Some(v) = sample.velocity {
            let speed = v.magnitude();
            if speed.is_finite() {
                speed_sum += speed as f64;
                speed_samples += 1;
                if speed < min_speed {
                    min_speed = speed;
                }
                if speed > max_speed {
                    max_speed = speed;
                }
            }
        }
        for w in &warnings {
            warning_count += 1;
            let key = warning_kind_key(w);
            *warnings_by_kind.entry(key).or_insert(0) += 1;
        }
        samples.push(sample);
        packet_count += 1;
        if packet_count.is_multiple_of(500) {
            progress_cb(packet_count);
        }
    }
    progress_cb(packet_count);

    let mean_speed = if speed_samples > 0 {
        (speed_sum / speed_samples as f64) as f32
    } else {
        0.0
    };
    let collision_events = detect_collisions(&samples, &CollisionConfig::default());
    let collision_count = collision_events.len() as u64;
    let collision_max_severity = collision_events
        .iter()
        .map(|event| event.severity)
        .max()
        .unwrap_or(0);
    let collision_avg_severity = if collision_events.is_empty() {
        None
    } else {
        Some(
            collision_events
                .iter()
                .map(|event| event.severity as f32)
                .sum::<f32>()
                / collision_events.len() as f32,
        )
    };
    let summary = PipelineSummary {
        packet_count,
        sample_count: samples.len() as u64,
        warning_count,
        warnings_by_kind,
        schema_endpoint: schema.endpoint.clone(),
        schema_field_count: schema.fields.len(),
        schema_config_hash: schema.config_hash.clone(),
        mean_speed,
        max_speed: if max_speed.is_finite() {
            max_speed
        } else {
            0.0
        },
        min_speed: if min_speed.is_finite() {
            min_speed
        } else {
            0.0
        },
        collision_count,
        collision_max_severity,
        collision_avg_severity,
        start_monotonic_ns: start_monotonic.unwrap_or(0),
    };
    Ok(PipelineOutput {
        summary,
        samples,
        collision_events,
    })
}

fn warning_kind_key(w: &ParserWarning) -> String {
    match w {
        ParserWarning::LengthShort { .. } => "length_short".to_string(),
        ParserWarning::UnexpectedTrailingBytes { .. } => "unexpected_trailing_bytes".to_string(),
        ParserWarning::NonFiniteValue { .. } => "non_finite_value".to_string(),
        ParserWarning::MotorRpmTruncated { .. } => "motor_rpm_truncated".to_string(),
    }
}
