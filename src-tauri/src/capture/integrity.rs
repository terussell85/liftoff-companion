use std::fs::File;
use std::io::{BufReader, Read};
use std::path::Path;

use crate::error::{AppError, AppResult};

pub fn compute_file_hash(path: &Path) -> AppResult<String> {
    let file = File::open(path)?;
    let mut reader = BufReader::new(file);
    let mut hasher = blake3::Hasher::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = reader.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

pub fn verify_file_hash(path: &Path, expected: &str) -> AppResult<()> {
    let actual = compute_file_hash(path)?;
    if actual != expected {
        return Err(AppError::HashMismatch {
            expected: expected.to_string(),
            actual,
        });
    }
    Ok(())
}

pub fn hash_bytes(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}
