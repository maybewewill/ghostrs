#![forbid(unsafe_code)]

mod archive;
mod block_table;
mod compression;
mod crypt;
mod error;
mod hash_table;
mod header;

pub use archive::{Archive, MpqFile};
pub use block_table::MpqBlockEntry;
pub use crypt::{hash_string, HASH_FILE_KEY, HASH_NAME_A, HASH_NAME_B, HASH_TABLE_OFFSET};
pub use error::MpqError;
pub use hash_table::MpqHashEntry;
pub use header::MpqHeader;
