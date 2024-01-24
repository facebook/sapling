/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::fs::File;
#[cfg(unix)]
use std::fs::Permissions;
use std::io;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use tempfile::NamedTempFile;

/// Create a temp file and then rename it into the specified path to
/// achieve atomicity. The temp file is created in the same directory
/// as path to ensure the rename is not cross filesystem. If fysnc is
/// true, the file will be fsynced before and after renaming, and the
/// directory will by fsynced after renaming.
///
/// mode_perms is required but does nothing on windows. mode_perms is
/// not automatically umasked.
///
/// The renamed file is returned. Any further data written to the file
/// will not be atomic since the file is already visibile to readers.
///
/// Note that the rename operation will fail on windows if the
/// destination file exists and is open.
pub fn atomic_write(
    path: &Path,
    #[allow(dead_code)] mode_perms: u32,
    fsync: bool,
    op: impl FnOnce(&mut File) -> io::Result<()>,
) -> io::Result<File> {
    let mut af = AtomicFile::open(path, mode_perms, fsync)?;
    op(af.as_file())?;
    af.save()
}

/// State to wait for change to a path.
pub struct Wait<'a> {
    path: &'a Path,
    meta: Option<fs::Metadata>,
}

impl<'a> Wait<'a> {
    /// Construct from a path for change detection. This reads the
    /// file stats immediately. If you also need to read the file
    /// content to double check whether to "wait" or not, call
    /// this before reading the file content to avoid races.
    pub fn from_path(path: &'a Path) -> io::Result<Self> {
        let meta = match path.symlink_metadata() {
            Ok(m) => Some(m),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        Ok(Self { path, meta })
    }

    /// Wait for change on the given `path`. The `path` is expected to be
    /// atomically updated (ex. `inode` should change on Linux).
    ///
    /// If `path` does not exist, wait for it to be created.
    pub fn wait_for_change(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        use std::os::unix::fs::MetadataExt;
        #[cfg(windows)]
        use std::os::windows::fs::MetadataExt;
        tracing::debug!("waiting for atomic change: {}", self.path.display());
        let mut new_wait;
        'wait_loop: loop {
            new_wait = Self::from_path(self.path)?;
            match (&self.meta, new_wait.meta.as_ref()) {
                (None, None) => {}
                (Some(_), None) | (None, Some(_)) => {
                    tracing::trace!(" waited: existence changed");
                    break 'wait_loop;
                }
                (Some(new), Some(old)) => {
                    #[cfg(unix)]
                    if new.ino() != old.ino() {
                        tracing::trace!(" waited: inode changed");
                        break 'wait_loop;
                    }
                    // Consider using `file_index`, similar to `ino` once stabilized:
                    // https://github.com/rust-lang/rust/issues/63010
                    #[cfg(windows)]
                    if new.last_write_time() != old.last_write_time()
                        || new.creation_time() != old.creation_time()
                    {
                        tracing::trace!(" waited: mtime changed");
                        break 'wait_loop;
                    }
                }
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        self.meta = new_wait.meta;
        Ok(())
    }
}

pub struct AtomicFile {
    file: NamedTempFile,
    path: PathBuf,
    dir: PathBuf,
    fsync: bool,
}

impl AtomicFile {
    pub fn open(
        path: &Path,
        #[allow(unused_variables)] mode_perms: u32,
        fsync: bool,
    ) -> io::Result<Self> {
        let dir = match path.parent() {
            Some(dir) => dir,
            None => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
        };

        #[allow(unused_mut)]
        let mut temp = NamedTempFile::new_in(dir)?;

        #[cfg(unix)]
        {
            let f = temp.as_file_mut();
            f.set_permissions(Permissions::from_mode(mode_perms))?;
        }

        Ok(Self {
            file: temp,
            path: path.to_path_buf(),
            dir: dir.to_path_buf(),
            fsync,
        })
    }

    pub fn as_file(&mut self) -> &mut File {
        self.file.as_file_mut()
    }

    pub fn save(self) -> io::Result<File> {
        #[allow(unused_variables)]
        let (mut temp, path, dir, fsync) = (self.file, self.path, self.dir, self.fsync);
        let f = temp.as_file_mut();

        if fsync {
            f.sync_data()?;
        }

        let max_retries = if cfg!(windows) { 5u16 } else { 0 };
        let mut retry = 0;
        loop {
            match temp.persist(&path) {
                Ok(persisted) => {
                    if fsync {
                        persisted.sync_all()?;

                        // Also sync the directory on Unix.
                        // Windows does not support syncing a directory.
                        #[cfg(unix)]
                        {
                            if let Ok(opened) = fs::OpenOptions::new().read(true).open(dir) {
                                let _ = opened.sync_all();
                            }
                        }
                    }

                    break Ok(persisted);
                }
                Err(e) => {
                    if retry == max_retries || e.error.kind() != io::ErrorKind::PermissionDenied {
                        break Err(e.error);
                    }

                    // Windows fails with "Access Denied" if destination file is open.
                    // Retry a few times.
                    tracing::info!(
                        retry,
                        ?path,
                        "atomic_write rename failed with EPERM. Will retry.",
                    );
                    std::thread::sleep(std::time::Duration::from_millis(1 << retry));
                    temp = e.file;

                    retry += 1;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write;
    #[cfg(unix)]
    use std::os::unix::prelude::MetadataExt;
    use std::sync::mpsc;

    use tempfile::tempdir;

    use super::*;

    #[test]
    fn test_atomic_write() -> io::Result<()> {
        let td = tempdir()?;

        let foo_path = td.path().join("foo");
        atomic_write(&foo_path, 0o640, false, |f| {
            f.write_all(b"sushi")?;
            Ok(())
        })?;

        // Sanity check that we wrote contents and the temp file is gone.
        assert_eq!("sushi", std::fs::read_to_string(&foo_path)?);
        assert_eq!(1, std::fs::read_dir(td.path())?.count());

        // Make sure we can set the mode perms on unix.
        #[cfg(unix)]
        assert_eq!(
            0o640,
            0o777 & std::fs::File::open(&foo_path)?.metadata()?.mode()
        );

        Ok(())
    }

    #[test]
    fn test_wait_for_change() -> io::Result<()> {
        let dir = tempdir()?;
        let path = dir.path().join("f");

        let (tx, rx) = mpsc::channel::<i32>();

        std::thread::spawn({
            let path = path.clone();
            move || {
                let mut wait = Wait::from_path(&path).unwrap();

                wait.wait_for_change().unwrap();
                tx.send(101).unwrap();

                wait.wait_for_change().unwrap();
                tx.send(102).unwrap();

                wait.wait_for_change().unwrap();
                tx.send(103).unwrap();
            }
        });

        // Nothing changed yet.
        std::thread::sleep(Duration::from_millis(110));
        assert!(rx.try_recv().is_err());

        // Create.
        atomic_write(&path, 0o640, false, |_| Ok(()))?;
        assert_eq!(rx.recv().unwrap(), 101);

        // Rewrite.
        atomic_write(&path, 0o640, false, |_| Ok(()))?;
        assert_eq!(rx.recv().unwrap(), 102);

        // Delete.
        std::fs::remove_file(&path)?;
        assert_eq!(rx.recv().unwrap(), 103);

        Ok(())
    }
}
