use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Vec3 {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Quaternion {
    pub x: f32,
    pub y: f32,
    pub z: f32,
    pub w: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct ControlInput {
    pub throttle: f32,
    pub yaw: f32,
    pub pitch: f32,
    pub roll: f32,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, Default)]
pub struct Battery {
    pub voltage: f32,
    pub percentage: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TelemetrySample {
    pub sequence_number: u64,
    pub capture_time_seconds: f64,
    pub sim_time: Option<f32>,
    pub position: Option<Vec3>,
    pub attitude: Option<Quaternion>,
    pub velocity: Option<Vec3>,
    pub gyro: Option<Vec3>,
    pub input: Option<ControlInput>,
    pub battery: Option<Battery>,
    pub motor_rpm: Option<Vec<f32>>,
    pub raw_packet_len: usize,
}

impl Vec3 {
    pub fn magnitude(&self) -> f32 {
        (self.x * self.x + self.y * self.y + self.z * self.z).sqrt()
    }
}
