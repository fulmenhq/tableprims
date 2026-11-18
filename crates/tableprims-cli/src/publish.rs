// SPDX-License-Identifier: MIT OR Apache-2.0
//! Publishing a file under a name that must not already exist.

use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};

/// A temporary file beside its destination, removed when dropped unless it
/// was published.
pub(crate) struct Pending {
    path: PathBuf,
    target: PathBuf,
    published: bool,
}

impl Pending {
    /// Creates a new, empty temporary file in the destination's directory,
    /// named `.tablebolt-<token>.<role>.tmp`. `token` makes the name unique
    /// to this run and `role` to the file within the run. The name does not
    /// grow with the destination's.
    pub(crate) fn create(target: &Path, token: &str, role: &str) -> io::Result<(Self, File)> {
        if target.file_name().is_none() {
            return Err(io::ErrorKind::InvalidInput.into());
        }
        let path = target.with_file_name(format!(".tablebolt-{token}.{role}.tmp"));
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)?;
        let pending = Self {
            path,
            target: target.to_owned(),
            published: false,
        };
        Ok((pending, file))
    }

    /// Gives the temporary file its destination name. Fails with
    /// [`io::ErrorKind::AlreadyExists`] when the destination exists; an
    /// existing file is never replaced.
    pub(crate) fn publish(mut self) -> io::Result<Published> {
        let published = Published {
            identity: identity(&self.path),
            path: self.target.clone(),
        };
        match fs::hard_link(&self.path, &self.target) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => return Err(error),
            Err(_) => {
                // No hard links here. Claim the name, then move the file
                // onto the claim.
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&self.target)?;
                if let Err(error) = fs::rename(&self.path, &self.target) {
                    let _ = fs::remove_file(&self.target);
                    return Err(error);
                }
                self.published = true;
                sync_directory(&self.target);
                return Ok(published);
            }
        }
        self.published = true;
        let _ = fs::remove_file(&self.path);
        sync_directory(&self.target);
        Ok(published)
    }
}

/// What identifies a file apart from its name, where the system has such a
/// thing.
#[cfg(unix)]
fn identity(path: &Path) -> Option<(u64, u64)> {
    use std::os::unix::fs::MetadataExt;
    fs::symlink_metadata(path)
        .ok()
        .map(|metadata| (metadata.dev(), metadata.ino()))
}

#[cfg(not(unix))]
fn identity(_: &Path) -> Option<(u64, u64)> {
    None
}

/// A file this run published.
pub(crate) struct Published {
    path: PathBuf,
    identity: Option<(u64, u64)>,
}

impl Published {
    /// Removes the file if the name still refers to the file this run
    /// published. Returns whether it was removed. Where files have no
    /// identity apart from their name, nothing is removed.
    pub(crate) fn withdraw(&self) -> bool {
        self.identity.is_some()
            && identity(&self.path) == self.identity
            && fs::remove_file(&self.path).is_ok()
    }
}

/// Whether two paths name the same directory entry: the same file name in
/// the same directory, however the directory is spelled.
pub(crate) fn same_entry(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }
    let resolved = |path: &Path| fs::canonicalize(directory(path)).ok();
    left.file_name().is_some()
        && left.file_name() == right.file_name()
        && resolved(left).is_some()
        && resolved(left) == resolved(right)
}

impl Drop for Pending {
    fn drop(&mut self) {
        if !self.published {
            let _ = fs::remove_file(&self.path);
        }
    }
}

/// The directory a file lives in.
pub(crate) fn directory(path: &Path) -> &Path {
    match path.parent() {
        Some(parent) if !parent.as_os_str().is_empty() => parent,
        _ => Path::new("."),
    }
}

/// Asks the system to make a new directory entry durable, where it can.
fn sync_directory(path: &Path) {
    #[cfg(unix)]
    if let Ok(handle) = File::open(directory(path)) {
        let _ = handle.sync_all();
    }
    #[cfg(not(unix))]
    let _ = path;
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn scratch(name: &str) -> PathBuf {
        let path =
            std::env::temp_dir().join(format!("tablebolt-publish-{name}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }

    #[test]
    fn a_published_file_has_its_content_and_no_temporary_remains() {
        let dir = scratch("ok");
        let target = dir.join("out.bin");
        let (pending, mut file) = Pending::create(&target, "t1", "workbook").unwrap();
        assert!(dir.join(".tablebolt-t1.workbook.tmp").exists());
        file.write_all(b"content").unwrap();
        drop(file);
        let published = pending.publish().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"content");
        assert!(same_entry(&target, &dir.join(".").join("out.bin")));
        assert!(!same_entry(&target, &dir.join("other.bin")));
        // A name that now refers to another file is left alone.
        if cfg!(unix) {
            fs::remove_file(&target).unwrap();
            fs::write(dir.join("keep"), b"x").unwrap();
            fs::write(&target, b"theirs").unwrap();
            assert!(!published.withdraw());
            assert_eq!(fs::read(&target).unwrap(), b"theirs");
        }
        #[cfg(unix)]
        {
            let (pending, file) = Pending::create(&dir.join("mine.bin"), "t4", "workbook").unwrap();
            drop(file);
            let mine = pending.publish().unwrap();
            assert!(mine.withdraw());
            assert!(!dir.join("mine.bin").exists());
        }
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_existing_destination_is_kept() {
        let dir = scratch("exists");
        let target = dir.join("out.bin");
        fs::write(&target, b"theirs").unwrap();
        let (pending, mut file) = Pending::create(&target, "t2", "workbook").unwrap();
        file.write_all(b"ours").unwrap();
        drop(file);
        let error = pending.publish().map(|_| ()).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"theirs");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unpublished_temporary_is_removed() {
        let dir = scratch("dropped");
        let name = "n".repeat(255);
        let (pending, file) = Pending::create(&dir.join(&name), "t3", "result").unwrap();
        drop(file);
        drop(pending);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
