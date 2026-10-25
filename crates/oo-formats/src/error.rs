use std::io;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),
    #[error("not a {expected} file (magic was {found:?})")]
    BadMagic {
        expected: &'static str,
        found: [u8; 4],
    },
    #[error("unsupported {format} version {version}")]
    UnsupportedVersion { format: &'static str, version: u32 },
    #[error("file is truncated or corrupt: {0}")]
    Corrupt(String),
    #[error("no such file in archive: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, Error>;
