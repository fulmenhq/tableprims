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

    /// Gives the temporary file its destination name by linking it there,
    /// which the system refuses when the name exists. Fails with
    /// [`io::ErrorKind::AlreadyExists`] when the destination exists; an
    /// existing file is never replaced. There is no other way to publish:
    /// on any failure the destination is left as it was.
    pub(crate) fn publish(mut self) -> io::Result<()> {
        fs::hard_link(&self.path, &self.target)?;
        self.published = true;
        let _ = fs::remove_file(&self.path);
        sync_directory(&self.target);
        Ok(())
    }
}

/// Whether a file in the destination's directory can be given a second name
/// by linking, which is how files are published. Two temporary files named
/// `.tablebolt-<token>.<role>-probe.tmp` and `…-probe-link.tmp` are created
/// and removed. `Ok(false)` is the file system declining to link.
pub(crate) fn links(target: &Path, token: &str, role: &str) -> io::Result<bool> {
    let (first, file) = Pending::create(target, token, &format!("{role}-probe"))?;
    drop(file);
    let second = target.with_file_name(format!(".tablebolt-{token}.{role}-probe-link.tmp"));
    let linked = fs::hard_link(&first.path, &second);
    let _ = fs::remove_file(&second);
    match linked {
        Ok(()) => Ok(true),
        // Causes that have nothing to do with linking are passed on.
        Err(error)
            if matches!(
                error.kind(),
                io::ErrorKind::StorageFull | io::ErrorKind::QuotaExceeded | io::ErrorKind::NotFound
            ) =>
        {
            Err(error)
        }
        // The directory accepted a new file a moment ago, so any other
        // answer is the file system declining to link. Systems name that
        // refusal in different ways, so a link denied by a security policy
        // is reported the same way.
        Err(_) => Ok(false),
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
        assert!(links(&target, "t1", "workbook").unwrap());
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        pending.publish().unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"content");
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 1);
        assert!(same_entry(&target, &dir.join(".").join("out.bin")));
        assert!(!same_entry(&target, &dir.join("other.bin")));
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
        let error = pending.publish().unwrap_err();
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
