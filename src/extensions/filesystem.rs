//! Private-store checks. Same-user hostile mutation is outside the native-extension trust model.
use std::{
    fs::{File, Metadata, OpenOptions},
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Component, Path},
};
use crate::ConfigError;
use super::invalid;

pub(super) fn no_links(path: &Path) -> Result<(), ConfigError> {
    if !path.is_absolute() || path.components().any(|c| matches!(c, Component::ParentDir)) {
        return Err(invalid());
    }
    for ancestor in path.ancestors() {
        if std::fs::symlink_metadata(ancestor).is_ok_and(|m| m.file_type().is_symlink()) {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn private_dir(path: &Path, owner: Option<u32>) -> Result<u32, ConfigError> {
    no_links(path)?;
    let meta = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    if !meta.is_dir() || meta.mode() & 0o077 != 0 || owner.is_some_and(|id| id != meta.uid()) {
        return Err(invalid());
    }
    Ok(meta.uid())
}

fn regular(meta: &Metadata, owner: u32) -> Result<(), ConfigError> {
    if !meta.is_file() || meta.nlink() != 1 || meta.uid() != owner || meta.mode() & 0o077 != 0 {
        return Err(invalid());
    }
    Ok(())
}

pub(super) fn open(path: &Path, owner: u32, write: bool) -> Result<File, ConfigError> {
    no_links(path)?;
    let before = std::fs::symlink_metadata(path).map_err(|_| invalid())?;
    regular(&before, owner)?;
    let file = OpenOptions::new().read(true).write(write).open(path).map_err(|_| invalid())?;
    let after = file.metadata().map_err(|_| invalid())?;
    regular(&after, owner)?;
    if (before.dev(), before.ino()) != (after.dev(), after.ino()) {
        return Err(invalid());
    }
    Ok(file)
}

pub(super) fn read(path: &Path, maximum: u64, owner: u32) -> Result<Vec<u8>, ConfigError> {
    let file = open(path, owner, false)?;
    if file.metadata().map_err(|_| invalid())?.len() > maximum {
        return Err(invalid());
    }
    let mut raw = Vec::new();
    file.take(maximum + 1).read_to_end(&mut raw).map_err(|_| invalid())?;
    if raw.len() as u64 > maximum {
        return Err(invalid());
    }
    Ok(raw)
}

pub(super) fn runtime_lock(root: &Path, owner: u32) -> Result<File, ConfigError> {
    let path = root.join(".runtime.lock");
    no_links(&path)?;
    match OpenOptions::new().read(true).write(true).create_new(true).mode(0o600).open(&path) {
        Ok(file) => drop(file),
        Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(invalid()),
    }
    let file = open(&path, owner, true)?;
    file.try_lock().map_err(|_| ConfigError("Extension store is already supervised or cannot be locked".into()))?;
    Ok(file)
}
