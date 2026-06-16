use byteorder::{LittleEndian, ReadBytesExt};
use serde::{Deserialize, Serialize};
use std::io::Cursor;

use crate::telemetry::liftoff_schema::{FieldKind, LiftoffSchema};
use crate::telemetry::sample::{Battery, ControlInput, Quaternion, TelemetrySample, Vec3};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ParserWarning {
    LengthShort {
        field: String,
        need: usize,
        have: usize,
    },
    UnexpectedTrailingBytes {
        bytes: usize,
    },
    NonFiniteValue {
        field: String,
    },
    MotorRpmTruncated {
        expected: usize,
        had: usize,
    },
}

pub fn parse_packet(
    bytes: &[u8],
    schema: &LiftoffSchema,
    sequence_number: u64,
    capture_time_seconds: f64,
) -> (TelemetrySample, Vec<ParserWarning>) {
    let mut sample = TelemetrySample {
        sequence_number,
        capture_time_seconds,
        raw_packet_len: bytes.len(),
        ..Default::default()
    };
    let mut warnings = Vec::new();
    let mut cursor = Cursor::new(bytes);

    for field in &schema.fields {
        let remaining = bytes.len() - cursor.position() as usize;
        if let Some(need) = field.fixed_size() {
            if remaining < need {
                warnings.push(ParserWarning::LengthShort {
                    field: field.name().to_string(),
                    need,
                    have: remaining,
                });
                return (sample, warnings);
            }
        }
        match field {
            FieldKind::Timestamp => {
                let t = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                if !t.is_finite() {
                    warnings.push(ParserWarning::NonFiniteValue {
                        field: "Timestamp".into(),
                    });
                }
                sample.sim_time = Some(t);
            }
            FieldKind::Position => {
                let x = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let y = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let z = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                if [x, y, z].iter().any(|v| !v.is_finite()) {
                    warnings.push(ParserWarning::NonFiniteValue {
                        field: "Position".into(),
                    });
                }
                sample.position = Some(Vec3 { x, y, z });
            }
            FieldKind::Attitude => {
                let x = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let y = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let z = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let w = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                if [x, y, z, w].iter().any(|v| !v.is_finite()) {
                    warnings.push(ParserWarning::NonFiniteValue {
                        field: "Attitude".into(),
                    });
                }
                sample.attitude = Some(Quaternion { x, y, z, w });
            }
            FieldKind::Velocity => {
                let x = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let y = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let z = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                if [x, y, z].iter().any(|v| !v.is_finite()) {
                    warnings.push(ParserWarning::NonFiniteValue {
                        field: "Velocity".into(),
                    });
                }
                sample.velocity = Some(Vec3 { x, y, z });
            }
            FieldKind::Gyro => {
                let x = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let y = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let z = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                sample.gyro = Some(Vec3 { x, y, z });
            }
            FieldKind::Input => {
                let throttle = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let yaw = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let pitch = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let roll = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                sample.input = Some(ControlInput {
                    throttle,
                    yaw,
                    pitch,
                    roll,
                });
            }
            FieldKind::Battery => {
                let voltage = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                let percentage = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                sample.battery = Some(Battery {
                    voltage,
                    percentage,
                });
            }
            FieldKind::MotorRpm => {
                let remaining = bytes.len() - cursor.position() as usize;
                if remaining < 1 {
                    warnings.push(ParserWarning::LengthShort {
                        field: "MotorRPM.count".into(),
                        need: 1,
                        have: remaining,
                    });
                    return (sample, warnings);
                }
                let count = cursor.read_u8().unwrap_or(0) as usize;
                let need = count * 4;
                let remaining = bytes.len() - cursor.position() as usize;
                let usable = remaining.min(need);
                let actual_count = usable / 4;
                if actual_count != count {
                    warnings.push(ParserWarning::MotorRpmTruncated {
                        expected: count,
                        had: actual_count,
                    });
                }
                let mut rpms = Vec::with_capacity(actual_count);
                for _ in 0..actual_count {
                    let v = cursor.read_f32::<LittleEndian>().unwrap_or(f32::NAN);
                    rpms.push(v);
                }
                sample.motor_rpm = Some(rpms);
            }
        }
    }

    let remaining = bytes.len() - cursor.position() as usize;
    if remaining > 0 {
        warnings.push(ParserWarning::UnexpectedTrailingBytes { bytes: remaining });
    }

    (sample, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::telemetry::liftoff_schema::{build_canonical_packet, LiftoffSchema};

    #[test]
    fn canonical_roundtrip() {
        let schema = LiftoffSchema::canonical("127.0.0.1:9001");
        let packet = build_canonical_packet(
            1.5,
            (1.0, 2.0, 3.0),
            (0.0, 0.0, 0.0, 1.0),
            (0.1, 0.2, 0.3),
            (10.0, 20.0, 30.0),
            (0.5, -0.1, 0.2, 0.0),
            (16.4, 92.0),
            &[1000.0, 1010.0, 990.0, 1005.0],
        );
        assert_eq!(packet.len(), 97);
        let (sample, warnings) = parse_packet(&packet, &schema, 7, 1.5);
        assert!(warnings.is_empty(), "unexpected warnings: {:?}", warnings);
        assert_eq!(sample.sequence_number, 7);
        assert_eq!(sample.sim_time, Some(1.5));
        let pos = sample.position.unwrap();
        assert_eq!((pos.x, pos.y, pos.z), (1.0, 2.0, 3.0));
        let att = sample.attitude.unwrap();
        assert_eq!((att.x, att.y, att.z, att.w), (0.0, 0.0, 0.0, 1.0));
        let battery = sample.battery.unwrap();
        assert_eq!(battery.voltage, 16.4);
        assert_eq!(battery.percentage, 92.0);
        assert_eq!(sample.motor_rpm.as_ref().unwrap().len(), 4);
    }

    #[test]
    fn short_packet_warns() {
        let schema = LiftoffSchema::canonical("127.0.0.1:9001");
        let packet = vec![0u8; 6];
        let (_sample, warnings) = parse_packet(&packet, &schema, 0, 0.0);
        assert!(matches!(warnings[0], ParserWarning::LengthShort { .. }));
    }
}
