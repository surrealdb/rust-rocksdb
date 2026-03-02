// Copyright (c) 2025-present, SurrealDB Ltd.  All rights reserved.

use crate::Error;

/// Encryption algorithm for a file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum EncryptionMethod {
    Unknown = 0,
    Plaintext = 1,
    Aes128Ctr = 2,
    Aes192Ctr = 3,
    Aes256Ctr = 4,
    Sm4Ctr = 5,
}

impl EncryptionMethod {
    pub(crate) fn from_raw(val: i32) -> Self {
        match val {
            1 => Self::Plaintext,
            2 => Self::Aes128Ctr,
            3 => Self::Aes192Ctr,
            4 => Self::Aes256Ctr,
            5 => Self::Sm4Ctr,
            _ => Self::Unknown,
        }
    }
}

/// Per-file encryption metadata returned by a [`KeyManager`].
#[derive(Debug, Clone)]
pub struct FileEncryptionInfo {
    pub method: EncryptionMethod,
    pub key: Vec<u8>,
    pub iv: Vec<u8>,
}

/// Trait for managing per-file encryption keys.
///
/// Implement this trait to control which encryption key and IV is used for
/// each RocksDB file. The encrypted env will call these methods whenever
/// it opens, creates, deletes, or links files.
///
/// All methods must be safe to call from multiple threads concurrently.
pub trait KeyManager: Send + Sync {
    /// Return the encryption info for an existing file.
    fn get_file(&self, fname: &str) -> Result<FileEncryptionInfo, Error>;

    /// Generate and return encryption info for a newly created file.
    fn new_file(&self, fname: &str) -> Result<FileEncryptionInfo, Error>;

    /// Called when a file has been deleted.
    fn delete_file(&self, fname: &str) -> Result<(), Error>;

    /// Called when a file has been hard-linked or copied.
    fn link_file(&self, src_fname: &str, dst_fname: &str) -> Result<(), Error>;
}
