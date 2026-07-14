//! Binary fingerprint for hello handshake and graceful replacement.
//!
//! Spec (`persistence-ipc.md` §8): `{ size, mtimeNs }`.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFingerprint {
    pub size: u64,
    pub mtime_ns: u64,
}

impl BinaryFingerprint {
    pub const ZERO: Self = Self {
        size: 0,
        mtime_ns: 0,
    };

    pub fn from_path(path: &Path) -> std::io::Result<Self> {
        let meta = std::fs::metadata(path)?;
        let size = meta.len();
        let mtime_ns = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0);
        Ok(Self { size, mtime_ns })
    }

    pub fn current() -> Self {
        current_exe_path()
            .and_then(|p| Self::from_path(&p))
            .unwrap_or(Self::ZERO)
    }
}

/// Absolute path of the running binary (best-effort).
pub fn current_exe_path() -> std::io::Result<PathBuf> {
    let path = std::env::current_exe()?;
    // Canonicalize when possible so hello carries a stable absolute path.
    std::fs::canonicalize(&path).or(Ok(path))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn fingerprint_reads_size_and_mtime() {
        let mut f = NamedTempFile::new().unwrap();
        write!(f, "hello-groktask").unwrap();
        f.flush().unwrap();
        let fp = BinaryFingerprint::from_path(f.path()).unwrap();
        assert_eq!(fp.size, 14);
        assert!(fp.mtime_ns > 0);
    }

    #[test]
    fn zero_constant() {
        assert_eq!(BinaryFingerprint::ZERO.size, 0);
        assert_eq!(BinaryFingerprint::ZERO.mtime_ns, 0);
    }
}
