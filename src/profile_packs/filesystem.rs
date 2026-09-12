//! Portable, bounded reads of nonsecret data. Digests bind the bytes actually read.
use super::invalid;
use crate::ConfigError;
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    path::{Component, Path, PathBuf},
};

pub(super) fn no_links(path: &Path) -> Result<(), ConfigError> {
    if path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid());
    }
    let absolute = std::path::absolute(path).map_err(|_| invalid())?;
    for ancestor in absolute.ancestors() {
        let meta = fs::symlink_metadata(ancestor).map_err(|_| invalid())?;
        if meta.file_type().is_symlink() {
            return Err(invalid());
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(invalid());
            }
        }
    }
    Ok(())
}

pub(super) fn read(path: &Path, maximum: u64) -> Result<Vec<u8>, ConfigError> {
    no_links(path)?;
    let meta = fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !meta.is_file() || meta.len() > maximum {
        return Err(invalid());
    }
    let file = File::open(path).map_err(|_| invalid())?;
    let meta = file.metadata().map_err(|_| invalid())?;
    if !meta.is_file() || meta.len() > maximum {
        return Err(invalid());
    }
    let mut raw = Vec::new();
    file.take(maximum + 1)
        .read_to_end(&mut raw)
        .map_err(|_| invalid())?;
    if raw.len() as u64 > maximum {
        return Err(invalid());
    }
    Ok(raw)
}

pub(super) fn directory(path: &Path) -> Result<(), ConfigError> {
    // The caller supplies an existing parent; do not traverse unverified new ancestors.
    if !path.exists() {
        no_links(path.parent().ok_or_else(invalid)?)?;
        match fs::create_dir(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(_) => return Err(invalid()),
        }
    }
    no_links(path)?;
    if !fs::metadata(path).map_err(|_| invalid())?.is_dir() {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn create(path: &Path, raw: &[u8]) -> Result<(), ConfigError> {
    no_links(path.parent().ok_or_else(invalid)?)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| invalid())?;
    if file.write_all(raw).and_then(|_| file.sync_all()).is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        return Err(invalid());
    }
    Ok(())
}

/// Serialize activation writers. An interrupted writer leaves an explicit recovery marker.
pub(super) struct Writer(PathBuf);
impl Writer {
    pub(super) fn acquire(root: &Path) -> Result<Self, ConfigError> {
        no_links(root)?;
        let path = root.join("activation.writer");
        create(&path, b"Interrupted writer: inspect active.json and activation.next before removing this marker.\n")?;
        Ok(Self(path))
    }
}
impl Drop for Writer {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(super) fn replace_activation(root: &Path, raw: &[u8]) -> Result<(), ConfigError> {
    let temporary = root.join("activation.next");
    let destination = root.join("active.json");
    if destination.exists() {
        no_links(&destination)?;
    }
    create(&temporary, raw)?;
    if fs::rename(&temporary, &destination).is_err() {
        let _ = fs::remove_file(temporary);
        return Err(invalid());
    }
    // Directory fsync is available on Unix. Windows replacement is atomic, but no
    // cross-platform power-loss durability claim is made for this administrative lock.
    #[cfg(unix)]
    File::open(root)
        .and_then(|file| file.sync_all())
        .map_err(|_| invalid())?;
    Ok(())
}
