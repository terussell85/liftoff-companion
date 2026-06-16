//! Liftoff telemetry packet schema.
//!
//! Liftoff serializes the fields configured in `TelemetryConfiguration.json`'s
//! `StreamFormat` array, in the order they appear. Sizes per field (little-endian):
//!
//!   Timestamp       = f32             (4 bytes)
//!   Position        = f32 × 3         (12 bytes)
//!   Attitude        = f32 × 4 (quat)  (16 bytes)
//!   Velocity        = f32 × 3         (12 bytes)
//!   Gyro            = f32 × 3         (12 bytes)
//!   Input           = f32 × 4         (16 bytes)
//!   Battery         = f32 × 2         (8 bytes)
//!   MotorRPM        = u8 + f32 × n    (1 + 4n bytes)
//!
//! Canonical config: all fields enabled in the order above. With a quad
//! (motor_count = 4) the packet is 97 bytes.

use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FieldKind {
    Timestamp,
    Position,
    Attitude,
    Velocity,
    Gyro,
    Input,
    Battery,
    MotorRpm,
}

impl FieldKind {
    pub fn from_name(name: &str) -> AppResult<Self> {
        match name {
            "Timestamp" => Ok(FieldKind::Timestamp),
            "Position" => Ok(FieldKind::Position),
            "Attitude" => Ok(FieldKind::Attitude),
            "Velocity" => Ok(FieldKind::Velocity),
            "Gyro" => Ok(FieldKind::Gyro),
            "Input" => Ok(FieldKind::Input),
            "Battery" => Ok(FieldKind::Battery),
            "MotorRPM" => Ok(FieldKind::MotorRpm),
            other => Err(AppError::LiftoffConfig(format!(
                "unknown StreamFormat field: {}",
                other
            ))),
        }
    }

    pub fn name(&self) -> &'static str {
        match self {
            FieldKind::Timestamp => "Timestamp",
            FieldKind::Position => "Position",
            FieldKind::Attitude => "Attitude",
            FieldKind::Velocity => "Velocity",
            FieldKind::Gyro => "Gyro",
            FieldKind::Input => "Input",
            FieldKind::Battery => "Battery",
            FieldKind::MotorRpm => "MotorRPM",
        }
    }

    /// Fixed-size fields return Some(bytes). MotorRPM returns None (variable).
    pub fn fixed_size(&self) -> Option<usize> {
        match self {
            FieldKind::Timestamp => Some(4),
            FieldKind::Position => Some(12),
            FieldKind::Attitude => Some(16),
            FieldKind::Velocity => Some(12),
            FieldKind::Gyro => Some(12),
            FieldKind::Input => Some(16),
            FieldKind::Battery => Some(8),
            FieldKind::MotorRpm => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiftoffSchema {
    pub fields: Vec<FieldKind>,
    pub endpoint: String,
    pub config_hash: String,
}

impl LiftoffSchema {
    pub fn canonical(endpoint: &str) -> Self {
        let fields = vec![
            FieldKind::Timestamp,
            FieldKind::Position,
            FieldKind::Attitude,
            FieldKind::Velocity,
            FieldKind::Gyro,
            FieldKind::Input,
            FieldKind::Battery,
            FieldKind::MotorRpm,
        ];
        let json = serde_json::json!({
            "EndPoint": endpoint,
            "StreamFormat": fields.iter().map(|f| f.name()).collect::<Vec<_>>(),
        });
        let canonical_bytes = serde_json::to_vec(&json).unwrap_or_default();
        let config_hash = crate::capture::integrity::hash_bytes(&canonical_bytes);
        Self {
            fields,
            endpoint: endpoint.to_string(),
            config_hash,
        }
    }

    pub fn from_config_json(json: &serde_json::Value) -> AppResult<Self> {
        let endpoint = json
            .get("EndPoint")
            .and_then(|v| v.as_str())
            .ok_or_else(|| AppError::LiftoffConfig("missing EndPoint".into()))?
            .to_string();
        let stream = json
            .get("StreamFormat")
            .and_then(|v| v.as_array())
            .ok_or_else(|| AppError::LiftoffConfig("missing StreamFormat".into()))?;
        let mut fields = Vec::with_capacity(stream.len());
        for v in stream {
            let s = v
                .as_str()
                .ok_or_else(|| AppError::LiftoffConfig("non-string StreamFormat entry".into()))?;
            fields.push(FieldKind::from_name(s)?);
        }
        let canonical_bytes = serde_json::to_vec(&serde_json::json!({
            "EndPoint": endpoint,
            "StreamFormat": fields.iter().map(|f| f.name()).collect::<Vec<_>>(),
        }))
        .unwrap_or_default();
        let config_hash = crate::capture::integrity::hash_bytes(&canonical_bytes);
        Ok(Self {
            fields,
            endpoint,
            config_hash,
        })
    }

    pub fn to_config_json(&self) -> serde_json::Value {
        serde_json::json!({
            "EndPoint": self.endpoint,
            "StreamFormat": self.fields.iter().map(|f| f.name()).collect::<Vec<_>>(),
        })
    }

    /// Total fixed-size bytes (excludes MotorRPM count + payload).
    pub fn fixed_total_bytes(&self) -> usize {
        self.fields
            .iter()
            .filter_map(|f| f.fixed_size())
            .sum::<usize>()
    }

    pub fn expects_motor_rpm(&self) -> bool {
        self.fields.contains(&FieldKind::MotorRpm)
    }
}

/// Convenience: build the canonical packet body for tests / synthetic emitters.
///
/// `motor_count` controls the number of MotorRPM floats appended.
#[allow(clippy::too_many_arguments)]
pub fn build_canonical_packet(
    timestamp: f32,
    position: (f32, f32, f32),
    attitude: (f32, f32, f32, f32),
    velocity: (f32, f32, f32),
    gyro: (f32, f32, f32),
    input: (f32, f32, f32, f32),
    battery: (f32, f32),
    motor_rpm: &[f32],
) -> Vec<u8> {
    use byteorder::{LittleEndian, WriteBytesExt};
    let mut buf = Vec::with_capacity(97);
    buf.write_f32::<LittleEndian>(timestamp).unwrap();
    buf.write_f32::<LittleEndian>(position.0).unwrap();
    buf.write_f32::<LittleEndian>(position.1).unwrap();
    buf.write_f32::<LittleEndian>(position.2).unwrap();
    buf.write_f32::<LittleEndian>(attitude.0).unwrap();
    buf.write_f32::<LittleEndian>(attitude.1).unwrap();
    buf.write_f32::<LittleEndian>(attitude.2).unwrap();
    buf.write_f32::<LittleEndian>(attitude.3).unwrap();
    buf.write_f32::<LittleEndian>(velocity.0).unwrap();
    buf.write_f32::<LittleEndian>(velocity.1).unwrap();
    buf.write_f32::<LittleEndian>(velocity.2).unwrap();
    buf.write_f32::<LittleEndian>(gyro.0).unwrap();
    buf.write_f32::<LittleEndian>(gyro.1).unwrap();
    buf.write_f32::<LittleEndian>(gyro.2).unwrap();
    buf.write_f32::<LittleEndian>(input.0).unwrap();
    buf.write_f32::<LittleEndian>(input.1).unwrap();
    buf.write_f32::<LittleEndian>(input.2).unwrap();
    buf.write_f32::<LittleEndian>(input.3).unwrap();
    buf.write_f32::<LittleEndian>(battery.0).unwrap();
    buf.write_f32::<LittleEndian>(battery.1).unwrap();
    buf.write_u8(motor_rpm.len() as u8).unwrap();
    for v in motor_rpm {
        buf.write_f32::<LittleEndian>(*v).unwrap();
    }
    buf
}
