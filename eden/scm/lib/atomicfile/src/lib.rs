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

pub struct AtomicFile {
    file: NamedTempFile,
    path: PathBuf,
    dir: PathBuf,
    fsync: bool,
}

impl AtomicFile {
    pub fn open(path: &Path, #[allow(dead_code)] mode_perms: u32, fsync: bool) -> io::Result<Self> {
        let dir = match path.parent() {
            Some(dir) => dir,
            None => return Err(io::Error::from(io::ErrorKind::InvalidInput)),
        };

        let mut temp = NamedTempFile::new_in(dir)?;
        let f = temp.as_file_mut();

        #[cfg(unix)]
        f.set_permissions(Permissions::from_mode(mode_perms))?;

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
}
