//! Filesystem checks for optional management adapters; these do not grant authority.
use crate::{Error, Result};
use std::{
    fs::{File, OpenOptions},
    path::{Component, Path},
};

pub fn directory(path: &Path) -> Result<()> {
    if !path.is_absolute() || path.components().any(|p| matches!(p, Component::ParentDir)) {
        return Err(Error::InvalidStore);
    }
    for ancestor in path.ancestors() {
        let meta = std::fs::symlink_metadata(ancestor).map_err(|_| Error::InvalidStore)?;
        if !meta.is_dir() || meta.file_type().is_symlink() {
            return Err(Error::InvalidStore);
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                return Err(Error::InvalidStore);
            }
        }
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        if std::fs::metadata(path)
            .map_err(|_| Error::InvalidStore)?
            .permissions()
            .mode()
            & 0o077
            != 0
        {
            return Err(Error::InvalidStore);
        }
    }
    Ok(())
}
pub fn regular(path: &Path) -> Result<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidStore)?;
    if !meta.is_file() || meta.file_type().is_symlink() {
        return Err(Error::InvalidStore);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let parent = std::fs::metadata(path.parent().ok_or(Error::InvalidStore)?)
            .map_err(|_| Error::InvalidStore)?;
        if meta.nlink() != 1 || meta.mode() & 0o077 != 0 || meta.uid() != parent.uid() {
            return Err(Error::InvalidStore);
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(Error::InvalidStore);
        }
    }
    Ok(())
}
pub fn private_new(path: &Path) -> Result<File> {
    let mut options = OpenOptions::new();
    options.read(true).write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options.open(path).map_err(|_| Error::Storage)
}
pub fn lease(path: &Path) -> Result<File> {
    directory(path.parent().ok_or(Error::InvalidStore)?)?;
    match private_new(path) {
        Ok(file) => drop(file),
        Err(_) if path.symlink_metadata().is_ok() => {}
        Err(error) => return Err(error),
    }
    regular(path)?;
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(path)
        .map_err(|_| Error::Storage)?;
    file.try_lock().map_err(|_| Error::AlreadyOwned)?;
    Ok(file)
}
