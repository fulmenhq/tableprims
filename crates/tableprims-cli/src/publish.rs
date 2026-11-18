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
    /// Creates a new, empty temporary file in the destination's directory.
    /// `token` makes the name unique to this run.
    pub(crate) fn create(target: &Path, token: &str) -> io::Result<(Self, File)> {
        let name = target.file_name().ok_or(io::ErrorKind::InvalidInput)?;
        let mut temporary = std::ffi::OsString::from(".");
        temporary.push(name);
        temporary.push(format!(".{token}.tmp"));
        let path = target.with_file_name(temporary);
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
    pub(crate) fn publish(mut self) -> io::Result<()> {
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
                return Ok(());
            }
        }
        self.published = true;
        let _ = fs::remove_file(&self.path);
        sync_directory(&self.target);
        Ok(())
    }
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
        let (pending, mut file) = Pending::create(&target, "t1").unwrap();
        file.write_all(b"content").unwrap();
        drop(file);
        pending.publish().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"content");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_existing_destination_is_kept() {
        let dir = scratch("exists");
        let target = dir.join("out.bin");
        fs::write(&target, b"theirs").unwrap();
        let (pending, mut file) = Pending::create(&target, "t2").unwrap();
        file.write_all(b"ours").unwrap();
        drop(file);
        let error = pending.publish().unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(fs::read(&target).unwrap(), b"theirs");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn an_unpublished_temporary_is_removed() {
        let dir = scratch("dropped");
        let (pending, file) = Pending::create(&dir.join("out.bin"), "t3").unwrap();
        drop(file);
        drop(pending);
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0);
        fs::remove_dir_all(&dir).unwrap();
    }
}
