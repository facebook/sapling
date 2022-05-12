/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::error;
use std::fmt;
use std::fs;
use std::fs::File;
use std::fs::OpenOptions;
use std::fs::Permissions;
use std::io;
use std::io::Write;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use fs2::FileExt;
use util::lock::PathLock;

/// try_lock attempts to acquire an advisory file lock and write
/// specified contents. Lock acquisition and content writing are
/// atomic as long as the content reader also uses this method. If
/// the lock is not available, LockContendederror is returned
/// immediately containing the lock's current contents.
pub fn try_lock(dir: &Path, name: &str, contents: &[u8]) -> anyhow::Result<File, LockError> {
    // Our locking strategy uses three files:
    //   1. An empty advisory lock file at the directory level.
    //   2. An empty advisory lock file named <name>.lock. This file is returned.
    //   3. A plain file named <name>.data which contains the contents.
    //
    //  Readers and writers acquire the directory lock first. This
    //  ensures atomicity across lock acquisition and content
    //  writing.
    let _dir_lock = PathLock::exclusive(dir.join(".dir_lock"))?;

    #[cfg(unix)]
    let _ = _dir_lock
        .as_file()
        .set_permissions(Permissions::from_mode(0o666));

    let name = sanitize_lock_name(name);

    let path = dir.join(name).with_extension("data");
    let lock_file = OpenOptions::new()
        .write(true)
        .create(true)
        .open(path.with_extension("lock"))?;

    #[cfg(unix)]
    let _ = lock_file.set_permissions(Permissions::from_mode(0o666));

    match lock_file.try_lock_exclusive() {
        Ok(_) => {}
        Err(err) if err.kind() == fs2::lock_contended_error().kind() => {
            let contents = fs::read(&path)?;
            return Err(LockContendedError { path, contents }.into());
        }
        Err(err) => return Err(err.into()),
    };

    let mut contents_file = File::create(path)?;
    #[cfg(unix)]
    let _ = contents_file.set_permissions(Permissions::from_mode(0o666));
    contents_file.write_all(contents.as_ref())?;

    Ok(lock_file)
}

fn sanitize_lock_name(name: &str) -> String {
    // Avoid letting a caller specify "foo.lock" and accidentally
    // interfering with the underlying locking details. This is
    // mainly for compatibility during python lock transition to
    // avoid a python lock "foo.lock" accidentally colliding with
    // the rust lock file.
    name.replace('.', "_")
}

#[derive(thiserror::Error, Debug)]
pub enum LockError {
    #[error(transparent)]
    Contended(#[from] LockContendedError),
    #[error(transparent)]
    Io(#[from] io::Error),
}

#[derive(Debug)]
pub struct LockContendedError {
    pub path: PathBuf,
    pub contents: Vec<u8>,
}

impl error::Error for LockContendedError {}

impl fmt::Display for LockContendedError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "lock {:?} contended", self.path)
    }
}

#[cfg(test)]
mod tests {
    use std::thread;

    use anyhow::Result;

    use super::*;

    #[test]
    fn test_try_lock() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        {
            let _foo_lock = try_lock(tmp.path(), "foo", "some contents".as_bytes())?;

            // Can get current lock data via lock contended error.
            if let Err(LockError::Contended(LockContendedError { contents, .. })) =
                try_lock(tmp.path(), "foo", "bar".as_bytes())
            {
                assert_eq!("some contents".as_bytes(), contents);
            } else {
                panic!("expected LockContendedError")
            }
        }

        // Now we can acquire "foo" lock since above lock has been dropped.
        let _foo_lock = try_lock(tmp.path(), "foo", "some contents".as_bytes())?;

        Ok(())
    }

    #[test]
    #[cfg(unix)]
    fn test_try_lock_permissions() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        try_lock(tmp.path(), "foo", "some contents".as_bytes())?;

        let assert_666 = |name: &str| {
            assert_eq!(
                tmp.path()
                    .join(name)
                    .metadata()
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o666
            );
        };

        assert_666(".dir_lock");
        assert_666("foo.lock");
        assert_666("foo.data");

        Ok(())
    }

    // Test readers never see incomplete or inconsistent lock data
    // contents.
    #[test]
    fn test_lock_atomicity() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        // Two threads taking turns with the lock. If lock is
        // unavailable, the thread knows the lock contents should be
        // that of the other thread (because there are only two
        // threads).
        let threads: Vec<_> = vec!["a", "b"]
            .into_iter()
            .map(|c| {
                // Make contents big so we include the case where
                // writing the contents takes multiple writes.
                let my_contents = c.repeat(1_000_000);
                let other = if c == "a" { "b" } else { "a" };
                let other_contents = other.repeat(1_000_000);
                let path = tmp.path().to_path_buf();
                thread::spawn(move || {
                    for _ in 0..10 {
                        match try_lock(&path, "foo", my_contents.as_bytes()) {
                            Ok(_) => {}
                            Err(LockError::Contended(LockContendedError { contents, .. })) => {
                                assert_eq!(other_contents.as_bytes(), contents);
                            }
                            _ => panic!("unexpected result"),
                        }
                    }
                })
            })
            .collect();

        for t in threads {
            t.join().unwrap();
        }

        Ok(())
    }
}
