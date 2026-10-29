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

            // Write beside the target and rename, so a concurrent reader never sees a half-written
            // object and two writers of the same bytes cannot interleave.
            let staging = path.with_extension(format!("tmp-{}", std::process::id()));
            fs::write(&staging, bytes).map_err(|e| Error::io(&staging, e))?;
            fs::rename(&staging, &path).map_err(|e| Error::io(&path, e))?;
        }

        Ok(format!("blake3:{hex}"))
    }

    /// Delete the payload behind an address, if it is there.
    pub fn remove(&self, address: &str) -> Result<()> {
        let path = self.path_for(hex_of(address)?);
        match fs::remove_file(&path) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(Error::io(&path, error)),
        }
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
        // Defensive: never slice past the end, even if a caller hands us a short address.
        let split = 2.min(hex.len());
        self.root.join(&hex[..split]).join(&hex[split..])
    }
}

fn hex_of(address: &str) -> Result<&str> {
    let hex = address
        .strip_prefix("blake3:")
        .ok_or_else(|| Error::InvalidObjectAddress {
            address: address.to_string(),
        })?;

    if hex.len() < 3 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::InvalidObjectAddress {
            address: address.to_string(),
        });
    }
    Ok(hex)
}
