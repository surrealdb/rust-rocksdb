// Copyright (c) 2025-present, SurrealDB Ltd.  All rights reserved.

mod encrypted_env;
mod key_manager;

pub use encrypted_env::create_encrypted_env;
pub use key_manager::{EncryptionMethod, FileEncryptionInfo, KeyManager};
