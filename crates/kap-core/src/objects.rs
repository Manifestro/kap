use std::fs;
use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

/// Content-addressed storage for payloads too large to keep in the database.
///
/// Objects are named by their BLAKE3 hash, so identical payloads are written once and a hash is
/// proof that two records saw the same bytes. Layout: `objects/<first two hex chars>/<rest>`.
#[derive(Debug, Clone)]
pub struct ObjectStore {
    root: PathBuf,
}

impl ObjectStore {
    /// Open the store rooted at `root`, without creating anything yet.
    pub fn new(root: PathBuf) -> ObjectStore {
        ObjectStore { root }
    }

    /// The directory objects are stored in.
    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Store `bytes` and return their address, e.g. `blake3:1f2e…`.
    ///
    /// Writing the same payload twice is a no-op: the second write finds the file already there.
    pub fn put(&self, bytes: &[u8]) -> Result<String> {
        let hash = blake3::hash(bytes);
        let hex = hash.to_hex().to_string();
        let path = self.path_for(&hex);

        if !path.exists() {
            if let Some(parent) = path.parent() {
                fs::create_dir_all(parent).map_err(|e| Error::io(parent, e))?;
            }
            fs::write(&path, bytes).map_err(|e| Error::io(&path, e))?;
        }

        Ok(format!("blake3:{hex}"))
    }

    /// Read the payload behind an address.
    pub fn get(&self, address: &str) -> Result<Vec<u8>> {
        let path = self.path_for(hex_of(address)?);
        fs::read(&path).map_err(|e| Error::io(&path, e))
    }

    /// Where the payload behind `address` lives.
    pub fn path(&self, address: &str) -> Result<PathBuf> {
        Ok(self.path_for(hex_of(address)?))
    }

    fn path_for(&self, hex: &str) -> PathBuf {
        self.root.join(&hex[..2]).join(&hex[2..])
    }
}

fn hex_of(address: &str) -> Result<&str> {
    address
        .strip_prefix("blake3:")
        .ok_or_else(|| Error::InvalidObjectAddress {
            address: address.to_string(),
        })
}
