//! Versioned raw capture file format.
//!
//! Layout:
//!   File header:
//!     magic: 8 bytes "WHOOPCAP"
//!     format_version: u16 LE
//!     header_len: u32 LE (size of the JSON header blob that follows)
//!     header_json: header_len bytes (UTF-8 JSON, see `FileHeader`)
//!
//!   Then repeated records:
//!     record_type: u8 (0 = packet)
//!     sequence_number: u64 LE
//!     monotonic_ns: u64 LE
//!     utc_ns: i64 LE (nanoseconds since Unix epoch, signed)
//!     addr_kind: u8 (4 = ipv4, 6 = ipv6)
//!     addr_bytes: 16 bytes (zero-padded for ipv4)
//!     port: u16 LE
//!     payload_len: u32 LE
//!     payload: payload_len bytes

use std::fs::File;
use std::io::{BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};
use std::path::Path;

use byteorder::{LittleEndian, ReadBytesExt, WriteBytesExt};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};

pub const MAGIC: &[u8; 8] = b"WHOOPCAP";
pub const FORMAT_VERSION: u16 = 1;
pub const RECORD_TYPE_PACKET: u8 = 0;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileHeader {
    pub format_version: u16,
    pub capture_id: String,
    pub created_at: String,
    pub app_version: String,
    pub telemetry_config_hash: Option<String>,
}

#[derive(Debug, Clone)]
pub struct PacketRecord {
    pub sequence_number: u64,
    pub monotonic_ns: u64,
    pub utc_ns: i64,
    pub source_addr: SocketAddr,
    pub payload: Vec<u8>,
}

pub struct RawCapWriter {
    file: BufWriter<File>,
    next_sequence: u64,
    bytes_written: u64,
}

impl RawCapWriter {
    pub fn create(path: &Path, header: &FileHeader) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let file = File::create(path)?;
        let mut writer = BufWriter::new(file);
        let header_json = serde_json::to_vec(header)?;
        let header_len = header_json.len() as u32;

        writer.write_all(MAGIC)?;
        writer.write_u16::<LittleEndian>(FORMAT_VERSION)?;
        writer.write_u32::<LittleEndian>(header_len)?;
        writer.write_all(&header_json)?;

        let bytes_written = MAGIC.len() as u64 + 2 + 4 + header_len as u64;
        Ok(Self {
            file: writer,
            next_sequence: 0,
            bytes_written,
        })
    }

    pub fn write_packet(
        &mut self,
        monotonic_ns: u64,
        utc_ns: i64,
        source_addr: SocketAddr,
        payload: &[u8],
    ) -> AppResult<u64> {
        let seq = self.next_sequence;
        self.next_sequence = self.next_sequence.wrapping_add(1);

        let mut addr_bytes = [0u8; 16];
        let addr_kind = match source_addr.ip() {
            IpAddr::V4(ip) => {
                let octets = ip.octets();
                addr_bytes[..4].copy_from_slice(&octets);
                4u8
            }
            IpAddr::V6(ip) => {
                addr_bytes.copy_from_slice(&ip.octets());
                6u8
            }
        };

        self.file.write_u8(RECORD_TYPE_PACKET)?;
        self.file.write_u64::<LittleEndian>(seq)?;
        self.file.write_u64::<LittleEndian>(monotonic_ns)?;
        self.file.write_i64::<LittleEndian>(utc_ns)?;
        self.file.write_u8(addr_kind)?;
        self.file.write_all(&addr_bytes)?;
        self.file.write_u16::<LittleEndian>(source_addr.port())?;
        self.file.write_u32::<LittleEndian>(payload.len() as u32)?;
        self.file.write_all(payload)?;

        let record_size = 1 + 8 + 8 + 8 + 1 + 16 + 2 + 4 + payload.len() as u64;
        self.bytes_written = self.bytes_written.saturating_add(record_size);
        Ok(self.bytes_written)
    }

    pub fn finalize(mut self) -> AppResult<u64> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(self.bytes_written)
    }
}

pub struct RawCapReader {
    file: BufReader<File>,
    header: FileHeader,
}

impl RawCapReader {
    pub fn open(path: &Path) -> AppResult<Self> {
        let file = File::open(path)?;
        let mut reader = BufReader::new(file);

        let mut magic = [0u8; 8];
        reader.read_exact(&mut magic)?;
        if &magic != MAGIC {
            return Err(AppError::CaptureFormat(format!("bad magic: {:?}", magic)));
        }

        let format_version = reader.read_u16::<LittleEndian>()?;
        if format_version != FORMAT_VERSION {
            return Err(AppError::CaptureFormat(format!(
                "unsupported format version: {}",
                format_version
            )));
        }

        let header_len = reader.read_u32::<LittleEndian>()? as usize;
        let mut header_buf = vec![0u8; header_len];
        reader.read_exact(&mut header_buf)?;
        let header: FileHeader = serde_json::from_slice(&header_buf)?;

        Ok(Self {
            file: reader,
            header,
        })
    }

    pub fn header(&self) -> &FileHeader {
        &self.header
    }

    pub fn next_record(&mut self) -> AppResult<Option<PacketRecord>> {
        let record_type = match self.file.read_u8() {
            Ok(b) => b,
            Err(e) if e.kind() == std::io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(e) => return Err(e.into()),
        };

        if record_type != RECORD_TYPE_PACKET {
            return Err(AppError::CaptureFormat(format!(
                "unknown record type: {}",
                record_type
            )));
        }

        let sequence_number = self.file.read_u64::<LittleEndian>()?;
        let monotonic_ns = self.file.read_u64::<LittleEndian>()?;
        let utc_ns = self.file.read_i64::<LittleEndian>()?;
        let addr_kind = self.file.read_u8()?;
        let mut addr_bytes = [0u8; 16];
        self.file.read_exact(&mut addr_bytes)?;
        let port = self.file.read_u16::<LittleEndian>()?;
        let payload_len = self.file.read_u32::<LittleEndian>()? as usize;
        let mut payload = vec![0u8; payload_len];
        self.file.read_exact(&mut payload)?;

        let ip = match addr_kind {
            4 => IpAddr::V4(Ipv4Addr::new(
                addr_bytes[0],
                addr_bytes[1],
                addr_bytes[2],
                addr_bytes[3],
            )),
            6 => IpAddr::V6(Ipv6Addr::from(addr_bytes)),
            other => {
                return Err(AppError::CaptureFormat(format!(
                    "unknown addr kind: {}",
                    other
                )))
            }
        };

        Ok(Some(PacketRecord {
            sequence_number,
            monotonic_ns,
            utc_ns,
            source_addr: SocketAddr::new(ip, port),
            payload,
        }))
    }

    pub fn count_packets(mut self) -> AppResult<u64> {
        let mut count = 0u64;
        while self.next_record()?.is_some() {
            count += 1;
        }
        Ok(count)
    }
}

pub fn read_header(path: &Path) -> AppResult<FileHeader> {
    let mut file = File::open(path)?;
    let mut magic = [0u8; 8];
    file.read_exact(&mut magic)?;
    if &magic != MAGIC {
        return Err(AppError::CaptureFormat(format!("bad magic: {:?}", magic)));
    }
    let _format_version = file.read_u16::<LittleEndian>()?;
    let header_len = file.read_u32::<LittleEndian>()? as usize;
    let mut header_buf = vec![0u8; header_len];
    file.read_exact(&mut header_buf)?;
    file.seek(SeekFrom::Start(0))?;
    let header: FileHeader = serde_json::from_slice(&header_buf)?;
    Ok(header)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{IpAddr, Ipv4Addr};

    #[test]
    fn roundtrip_packets() {
        let dir = tempdir_for_test();
        let path = dir.join("rt.rawcap");
        let header = FileHeader {
            format_version: FORMAT_VERSION,
            capture_id: "cap_test".to_string(),
            created_at: "2026-05-28T00:00:00Z".to_string(),
            app_version: "0.1.0".to_string(),
            telemetry_config_hash: Some("deadbeef".to_string()),
        };

        let mut writer = RawCapWriter::create(&path, &header).unwrap();
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 9001);
        for i in 0..50 {
            let payload = vec![i as u8; 32];
            writer
                .write_packet(i as u64 * 10_000, i as i64 * 1_000, addr, &payload)
                .unwrap();
        }
        let total_written = writer.finalize().unwrap();
        assert!(total_written > 0);

        let mut reader = RawCapReader::open(&path).unwrap();
        assert_eq!(reader.header().capture_id, "cap_test");
        let mut seen = 0u64;
        while let Some(rec) = reader.next_record().unwrap() {
            assert_eq!(rec.payload.len(), 32);
            assert_eq!(rec.source_addr, addr);
            assert_eq!(rec.sequence_number, seen);
            seen += 1;
        }
        assert_eq!(seen, 50);

        let reader = RawCapReader::open(&path).unwrap();
        assert_eq!(reader.count_packets().unwrap(), 50);
    }

    fn tempdir_for_test() -> std::path::PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!("whoop_test_{}", uuid::Uuid::new_v4().simple()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }
}
