use std::io;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MpqError {
    #[error("I/O error: {0}")]
    Io(#[from] io::Error),

    #[error("MPQ header not found in archive")]
    HeaderNotFound,

    #[error("Corrupted MPQ header: {0}")]
    CorruptedHeader(&'static str),

    #[error("Corrupted hash table in MPQ archive")]
    CorruptedHashTable,

    #[error("Corrupted block table in MPQ archive")]
    CorruptedBlockTable,

    #[error("File not found in MPQ archive: {0}")]
    FileNotFound(String),

    #[error("Decompression failed: {0}")]
    DecompressionFailed(String),

    #[error("Invalid sector table offset")]
    InvalidSectorTable,

    #[error("Unsupported compression method: 0x{0:02X}")]
    UnsupportedCompression(u8),

    #[error("Corrupted file data: expected {expected} bytes, got {got}")]
    UnexpectedEof { expected: usize, got: usize },
}
