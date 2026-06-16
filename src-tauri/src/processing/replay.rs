use std::path::Path;

use crate::capture::rawcap::RawCapReader;
use crate::error::AppResult;

pub fn count_packets(path: &Path) -> AppResult<u64> {
    let reader = RawCapReader::open(path)?;
    reader.count_packets()
}
