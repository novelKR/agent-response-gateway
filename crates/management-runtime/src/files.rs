use gateway_management::{Digest, Error, Result, filesystem};
use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

pub(crate) const MAX_CONFIG: u64 = 1024 * 1024;

pub(crate) fn read(path: &Path, limit: u64, private: bool) -> Result<Vec<u8>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error::InvalidInput);
    }
    for item in path.ancestors() {
        let meta = std::fs::symlink_metadata(item).map_err(|_| Error::InvalidStore)?;
        if meta.file_type().is_symlink() {
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
    let metadata = std::fs::metadata(path).map_err(|_| Error::InvalidStore)?;
    if !metadata.is_file() || metadata.len() > limit {
        return Err(Error::InvalidStore);
    }
    if private {
        filesystem::regular(path)?;
    }
    let mut bytes = vec![];
    File::open(path)
        .map_err(|_| Error::Storage)?
        .take(limit + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Storage)?;
    if bytes.len() as u64 > limit {
        return Err(Error::InvalidStore);
    }
    Ok(bytes)
}
pub(crate) fn hash_file(path: &Path) -> Result<Digest> {
    let bytes = read(path, 256 * 1024 * 1024, false)?;
    Ok(Digest::of(&bytes))
}
pub(crate) fn create(path: &Path, bytes: &[u8]) -> Result<()> {
    filesystem::directory(path.parent().ok_or(Error::InvalidStore)?)?;
    let mut file = filesystem::private_new(path)?;
    file.write_all(bytes)
        .and_then(|_| file.sync_all())
        .map_err(|_| Error::Storage)?;
    sync_parent(path)
}
pub(crate) fn replace(path: &Path, bytes: &[u8]) -> Result<()> {
    filesystem::directory(path.parent().ok_or(Error::InvalidStore)?)?;
    filesystem::regular(path)?;
    let temporary = path.with_file_name(format!(".{}.new", uuid::Uuid::new_v4()));
    create(&temporary, bytes)?;
    std::fs::rename(&temporary, path).map_err(|_| Error::Storage)?;
    sync_parent(path)
}
fn sync_parent(path: &Path) -> Result<()> {
    #[cfg(unix)]
    File::open(path.parent().ok_or(Error::InvalidStore)?)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::Storage)?;
    #[cfg(not(unix))]
    {
        let _ = path;
    }
    Ok(())
}
pub(crate) fn private_directory(path: &Path) -> Result<()> {
    let mut options = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        options.mode(0o700);
    }
    options.create(path).map_err(|_| Error::Storage)?;
    filesystem::directory(path)
}
pub(crate) fn complete_evidence(path: &Path, bytes: &[u8]) -> Result<()> {
    create(path, bytes)?;
    // Keep the writable handle requirement explicit for Windows durability.
    OpenOptions::new()
        .write(true)
        .open(path)
        .and_then(|f| f.sync_all())
        .map_err(|_| Error::Storage)
}
