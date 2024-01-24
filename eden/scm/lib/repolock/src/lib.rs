/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::error;
use std::fmt;
use std::fs::File;
#[cfg(unix)]
use std::fs::Permissions;
use std::io;
use std::io::Write;
use std::num::NonZeroU64;
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::thread::sleep;
use std::time::Duration;
use std::time::Instant;

use configmodel::Config;
use configmodel::ConfigExt;
use fs2::FileExt;
use parking_lot::Mutex;
use util::errors::IOContext;
use util::lock::PathLock;

const WORKING_COPY_NAME: &str = "wlock";
const STORE_NAME: &str = "lock";

pub struct RepoLocker {
    inner: Arc<Mutex<RepoLockerInner>>,
}

struct RepoLockerInner {
    config: LockConfigs,
    store_path: PathBuf,
    store_lock: Option<ReentrantLockHandle>,
    wc_locks: HashMap<PathBuf, ReentrantLockHandle>,
}

pub struct LockedPath {
    locker: Arc<Mutex<RepoLockerInner>>,
    store: bool,

    // If `store` is true, this is an ".hg/store" directory, else a working copy ".hg" directory.
    path: PathBuf,
}

impl std::ops::Deref for LockedPath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.path
    }
}

#[derive(Debug)]
struct ReentrantLockHandle {
    _lock: LockHandle,
    ref_count: NonZeroU64,
    // Was this lock acquired out of order, such as the latter of repo.lock()
    // followed by repo.wlock(wait=False).
    out_of_order: bool,
}

impl ReentrantLockHandle {
    pub fn new(lock: LockHandle, out_of_order: bool) -> Self {
        Self {
            _lock: lock,
            ref_count: NonZeroU64::new(1).unwrap(),
            out_of_order,
        }
    }

    pub fn inc_ref_count(&mut self) {
        self.ref_count = self.ref_count.checked_add(1).unwrap();
    }

    pub fn dec_ref_count(&mut self) {
        self.ref_count = NonZeroU64::new(self.ref_count.get() - 1).unwrap();
    }

    pub fn ref_count(&self) -> u64 {
        self.ref_count.get()
    }

    pub fn out_of_order(&self) -> bool {
        self.out_of_order
    }
}

#[derive(Debug)]
struct LockConfigs {
    pub deadline: Duration,
    pub warn_deadline: Duration,
    pub backoff: Duration,
}

impl LockConfigs {
    pub fn new(config: &dyn Config) -> Result<Self, LockError> {
        let deadline =
            Duration::from_secs_f64(config.get_or_default::<f64>("ui", "timeout")?.max(0_f64));

        let warn_deadline = Duration::from_secs_f64(
            config
                .get_or_default::<f64>("ui", "timeout.warn")?
                .max(0_f64),
        );

        let backoff = Duration::from_secs_f64(
            config
                .get_or::<f64>("devel", "lock_backoff", || 0.1)?
                .max(0_f64),
        );
        Ok(LockConfigs {
            deadline,
            warn_deadline,
            backoff,
        })
    }
}

impl RepoLocker {
    pub fn new(config: &dyn Config, store_path: PathBuf) -> Result<Self, LockError> {
        Ok(RepoLocker {
            inner: Arc::new(Mutex::new(RepoLockerInner {
                config: LockConfigs::new(config)?,
                store_path,
                store_lock: None,
                wc_locks: HashMap::new(),
            })),
        })
    }

    pub fn lock_store(&self) -> Result<LockedPath, LockError> {
        self.lock_store_maybe_wait(true)
    }

    pub fn try_lock_store(&self) -> Result<LockedPath, LockError> {
        self.lock_store_maybe_wait(false)
    }

    fn lock_store_maybe_wait(&self, wait: bool) -> Result<LockedPath, LockError> {
        let mut inner = self.inner.lock();
        inner.lock_store(wait)?;
        Ok(LockedPath::new_store_lock(
            self.inner.clone(),
            inner.store_path.clone(),
        ))
    }

    fn store_locked(&self) -> bool {
        self.inner.lock().store_lock.is_some()
    }

    pub fn lock_working_copy(&self, wc_dot_hg: PathBuf) -> Result<LockedPath, LockError> {
        self.lock_working_copy_maybe_wait(wc_dot_hg, true)
    }

    pub fn try_lock_working_copy(&self, wc_dot_hg: PathBuf) -> Result<LockedPath, LockError> {
        self.lock_working_copy_maybe_wait(wc_dot_hg, false)
    }

    fn lock_working_copy_maybe_wait(
        &self,
        wc_dot_hg: PathBuf,
        wait: bool,
    ) -> Result<LockedPath, LockError> {
        let mut inner = self.inner.lock();
        inner.lock_working_copy(wc_dot_hg.clone(), wait)?;
        Ok(LockedPath::new_working_copy_lock(
            self.inner.clone(),
            wc_dot_hg,
        ))
    }

    fn working_copy_locked(&self, wc_path: &Path) -> bool {
        self.inner.lock().wc_locks.contains_key(wc_path)
    }
}

impl RepoLockerInner {
    pub fn lock_store(&mut self, wait: bool) -> Result<(), LockError> {
        if let Some(store_lock) = &mut self.store_lock {
            store_lock.inc_ref_count();
        } else {
            let handle = if wait {
                lock(
                    &self.config,
                    &self.store_path,
                    STORE_NAME,
                    lock_contents()?.as_bytes(),
                )?
            } else {
                try_lock(&self.store_path, STORE_NAME, lock_contents()?.as_bytes())?
            };
            self.store_lock = Some(ReentrantLockHandle::new(handle, false));
        }
        Ok(())
    }

    pub fn lock_working_copy(&mut self, wc_dot_hg: PathBuf, wait: bool) -> Result<(), LockError> {
        if let Some(wc_lock) = self.wc_locks.get_mut(&wc_dot_hg) {
            wc_lock.inc_ref_count();
        } else {
            // TODO: Should we check that this working copy is actually related to this store?
            let handle = if wait {
                // Only check out-of-order if we are waiting. If we aren't
                // waiting and the the deadlock condition occurs (i.e. other
                // process locks wlock and is waiting for store lock), our wlock
                // acquisition will simply fail and no deadlock will occur.
                if self.store_lock.is_some() {
                    return Err(LockError::OutOfOrder(
                        "must not take store lock before wlock".to_string(),
                    ));
                }

                lock(
                    &self.config,
                    &wc_dot_hg,
                    WORKING_COPY_NAME,
                    lock_contents()?.as_bytes(),
                )?
            } else {
                try_lock(&wc_dot_hg, WORKING_COPY_NAME, lock_contents()?.as_bytes())?
            };

            self.wc_locks.insert(
                wc_dot_hg,
                ReentrantLockHandle::new(handle, self.store_lock.is_some()),
            );
        }
        Ok(())
    }
}

impl LockedPath {
    fn new_store_lock(locker: Arc<Mutex<RepoLockerInner>>, store_path: PathBuf) -> Self {
        LockedPath {
            locker,
            store: true,
            path: store_path,
        }
    }

    fn new_working_copy_lock(locker: Arc<Mutex<RepoLockerInner>>, wc_path: PathBuf) -> Self {
        LockedPath {
            locker,
            store: false,
            path: wc_path,
        }
    }

    /// Return ref count for this lock handle. Ref count of 1 means lock will be
    /// relased when this handle is dropped.
    pub fn count(&self) -> u64 {
        let locker = self.locker.lock();
        if self.store {
            locker.store_lock.as_ref().unwrap().ref_count()
        } else {
            locker.wc_locks.get(&self.path).unwrap().ref_count()
        }
    }
}

impl fmt::Debug for LockedPath {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        let locker = self.locker.lock();
        if self.store {
            if let Some(handle) = &locker.store_lock {
                fmt::Debug::fmt(&handle, f)?;
            } else {
                f.write_str("invalid store lock")?;
            }
        } else if let Some(handle) = locker.wc_locks.get(&self.path) {
            fmt::Debug::fmt(&handle, f)?;
        } else {
            f.write_fmt(format_args!("invalid wc lock {:?}", self.path))?;
        }
        Ok(())
    }
}

impl Drop for LockedPath {
    fn drop(&mut self) {
        let mut locker = self.locker.lock();
        if self.store {
            let store_lock = locker.store_lock.as_mut().unwrap();
            let lock_count = store_lock.ref_count();
            if lock_count > 1 {
                store_lock.dec_ref_count();
            } else {
                let _ = locker.store_lock.take();
            }
        } else {
            let wc_lock = locker.wc_locks.get_mut(&self.path).unwrap();
            let lock_count = wc_lock.ref_count();
            if lock_count > 1 {
                wc_lock.dec_ref_count();
            } else {
                if !wc_lock.out_of_order() && locker.store_lock.is_some() {
                    panic!("attempted to release wlock before lock");
                }
                locker.wc_locks.remove(&self.path);
            }
        }
    }
}

fn lock_contents() -> Result<String, LockError> {
    Ok(format!("{}:{}", util::sys::hostname()?, std::process::id()))
}

/// lock loops until it can acquire the specified lock, subject to
/// ui.timeout timeout. Errors other than lock contention are
/// propagated immediately with no retries.
#[tracing::instrument(skip_all, fields(name))]
fn lock(
    config: &LockConfigs,
    dir: &Path,
    name: &str,
    contents: &[u8],
) -> Result<LockHandle, LockError> {
    let start = Instant::now();

    loop {
        match try_lock(dir, name, contents) {
            Ok(h) => return Ok(h),
            Err(err) => match err {
                LockError::Contended(_) => {
                    // TODO: add user friendly debugging similar to Python locks.
                    let elapsed = start.elapsed();

                    if elapsed >= config.warn_deadline {
                        tracing::warn!(name, "lock contended");
                    } else {
                        tracing::info!(name, "lock contended");
                    };

                    if elapsed >= config.deadline {
                        return Err(err);
                    }

                    sleep(config.backoff)
                }
                _ => return Err(err),
            },
        }
    }
}

struct LockPaths {
    legacy: PathBuf,
    dir: PathBuf,
    data: PathBuf,
    lock: PathBuf,
}

impl LockPaths {
    pub fn new(dir: &Path, name: &str) -> Self {
        let dir_lock = dir.join(".dir_lock");
        let legacy = dir.join(name);

        let name = sanitize_lock_name(name);
        let data = dir.join(name).with_extension("data");
        let lock = data.with_extension("lock");

        Self {
            legacy,
            dir: dir_lock,
            data,
            lock,
        }
    }
}

/// try_lock attempts to acquire an advisory file lock and write
/// specified contents. Lock acquisition and content writing are
/// atomic as long as the content reader also uses this method. If
/// the lock is not available, LockContendederror is returned
/// immediately containing the lock's current contents.
pub fn try_lock(dir: &Path, name: &str, contents: &[u8]) -> Result<LockHandle, LockError> {
    let lock_paths = LockPaths::new(dir, name);

    // Our locking strategy uses three files:
    //   1. An empty advisory lock file at the directory level.
    //   2. An empty advisory lock file named <name>.lock. This file is returned.
    //   3. A plain file named <name>.data which contains the contents.
    //
    //  Readers and writers acquire the directory lock first. This
    //  ensures atomicity across lock acquisition and content
    //  writing.
    let _dir_lock = PathLock::exclusive(lock_paths.dir)?;

    #[cfg(unix)]
    let _ = _dir_lock
        .as_file()
        .set_permissions(Permissions::from_mode(0o666));

    let lock_file = util::file::open(&lock_paths.lock, "wc")?;

    #[cfg(unix)]
    let _ = lock_file.set_permissions(Permissions::from_mode(0o666));

    match lock_file.try_lock_exclusive() {
        Ok(_) => {}
        Err(err) if err.kind() == fs2::lock_contended_error().kind() => {
            let contents = fs_err::read(&lock_paths.data)?;
            return Err(LockContendedError {
                path: lock_paths.data,
                contents,
            }
            .into());
        }
        Err(err) => {
            return Err(util::errors::from_err_msg_path(
                err,
                "error locking lock file",
                &lock_paths.lock,
            )
            .into());
        }
    };

    // Create the legacy lock file to maintain compatibility for
    // external code that checks directly for .hg/wlock as an
    // indication of "is an hg operation in progress".
    if let Ok(mut legacy_file) = File::create(&lock_paths.legacy) {
        // Also write lock contents for compatibility with Python readers.
        let _ = legacy_file.write_all(contents.as_ref());

        #[cfg(unix)]
        let _ = legacy_file.set_permissions(Permissions::from_mode(0o644));
    }

    let mut contents_file = util::file::open(&lock_paths.data, "wct")?;
    #[cfg(unix)]
    let _ = contents_file.set_permissions(Permissions::from_mode(0o666));
    contents_file
        .write_all(contents.as_ref())
        .path_context("error write lock contents", &lock_paths.data)?;

    Ok(LockHandle {
        path: lock_paths.lock,
        lock: lock_file,
        legacy_path: lock_paths.legacy,
    })
}

#[derive(Debug)]
pub struct LockHandle {
    path: PathBuf,
    lock: File,
    legacy_path: PathBuf,
}

impl LockHandle {
    pub fn unlock(&mut self) -> io::Result<()> {
        self.unlink_legacy();
        self.lock
            .unlock()
            .path_context("error unlocking lock file", &self.path)
    }

    fn unlink_legacy(&mut self) {
        let _ = util::path::remove_file(&self.legacy_path);
    }
}

impl Drop for LockHandle {
    fn drop(&mut self) {
        self.unlink_legacy();
    }
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
    ConfigError(#[from] configmodel::Error),
    #[error(transparent)]
    Contended(#[from] LockContendedError),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("{0}")]
    OutOfOrder(String),
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

impl LockError {
    /// Test if the error is contended, aka. held by others.
    pub fn is_contended(&self) -> bool {
        matches!(self, LockError::Contended(_))
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::thread;
    use std::thread::spawn;

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

    #[test]
    fn test_lock_loop() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        let mut cfg = BTreeMap::from([("ui.timeout", "0.001"), ("devel.lock_backoff", "0.001")]);
        let lock_cfg = LockConfigs::new(&cfg)?;

        let first = lock(&lock_cfg, tmp.path(), "foo", "contents".as_bytes())?;

        assert!(matches!(
            lock(&lock_cfg, tmp.path(), "foo", "contents".as_bytes()),
            Err(LockError::Contended(_))
        ));

        cfg.insert("ui.timeout", "60");

        let lock_cfg = LockConfigs::new(&cfg)?;
        let dropper = spawn(move || {
            sleep(Duration::from_millis(5));
            drop(first);
        });

        assert!(lock(&lock_cfg, tmp.path(), "foo", "contents".as_bytes()).is_ok());

        dropper.join().unwrap();

        Ok(())
    }

    #[test]
    fn test_working_copy() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        let cfg = Box::<BTreeMap<&str, &str>>::default();

        let locker = RepoLocker::new(&cfg, tmp.path().to_path_buf())?;

        let _wlock = locker.lock_working_copy(tmp.path().to_path_buf())?;

        // Make sure locked the right file, and check the contents.
        match try_lock(tmp.path(), WORKING_COPY_NAME, "foo".as_bytes()) {
            Err(LockError::Contended(LockContendedError { contents, .. })) => {
                assert_eq!(
                    String::from_utf8(contents)?,
                    format!("{}:{}", util::sys::hostname()?, std::process::id())
                );
            }
            _ => panic!("lock should be contended"),
        };

        Ok(())
    }

    #[test]
    fn test_lock_order_verification() -> Result<()> {
        let store_tmp = tempfile::tempdir()?;
        let wc_tmp = tempfile::tempdir()?;

        let cfg = Box::<BTreeMap<&str, &str>>::default();

        let locker = RepoLocker::new(&cfg, store_tmp.path().to_path_buf())?;

        {
            let _wlock = locker.lock_working_copy(wc_tmp.path().to_path_buf())?;
            let _lock = locker.lock_store()?;
        }
        {
            let _lock = locker.lock_store()?;
            match locker.lock_working_copy(wc_tmp.path().to_path_buf()) {
                Err(LockError::OutOfOrder(_)) => {}
                result => panic!("wlock should be required before lock: {:?}", result),
            };
        }

        Ok(())
    }

    #[test]
    fn test_is_locked() -> Result<()> {
        let store_tmp = tempfile::tempdir()?;
        let wc_tmp = tempfile::tempdir()?;

        let cfg = BTreeMap::<&str, &str>::new();
        let locker = RepoLocker::new(&cfg, store_tmp.path().to_path_buf())?;

        assert!(!locker.working_copy_locked(wc_tmp.path()));
        assert!(!locker.store_locked());

        let _wlock = locker.lock_working_copy(wc_tmp.path().to_path_buf())?;
        assert!(locker.working_copy_locked(wc_tmp.path()));
        assert!(!locker.store_locked());

        let _lock = locker.lock_store()?;
        assert!(locker.working_copy_locked(wc_tmp.path()));
        assert!(locker.store_locked());

        Ok(())
    }

    #[test]
    fn test_taking_lock_twice() -> Result<()> {
        let store_tmp = tempfile::tempdir()?;
        let wc_tmp = tempfile::tempdir()?;

        let cfg = BTreeMap::<&str, &str>::new();
        let locker = RepoLocker::new(&cfg, store_tmp.path().to_path_buf())?;

        let _wclock1 = locker.lock_working_copy(wc_tmp.path().to_path_buf())?;
        let _wclock2 = locker.lock_working_copy(wc_tmp.path().to_path_buf())?;

        let _lock1 = locker.lock_store()?;
        let _lock2 = locker.lock_store()?;

        drop(_lock1);
        assert!(locker.store_locked());
        drop(_lock2);
        assert!(!locker.store_locked());
        drop(_wclock1);
        assert!(locker.working_copy_locked(wc_tmp.path()));
        drop(_wclock2);
        assert!(!locker.working_copy_locked(wc_tmp.path()));

        Ok(())
    }

    #[test]
    #[should_panic]
    fn test_bad_lock_release_order() {
        let store_tmp = tempfile::tempdir().unwrap();
        let wc_tmp = tempfile::tempdir().unwrap();

        let cfg = BTreeMap::<&str, &str>::new();
        let locker = RepoLocker::new(&cfg, store_tmp.path().to_path_buf()).unwrap();

        let _wclock1 = locker
            .lock_working_copy(wc_tmp.path().to_path_buf())
            .unwrap();
        let _wclock2 = locker
            .lock_working_copy(wc_tmp.path().to_path_buf())
            .unwrap();

        let _lock1 = locker.lock_store().unwrap();
        let _lock2 = locker.lock_store().unwrap();

        drop(_wclock1);
        drop(_wclock2);
    }

    #[test]
    fn test_interesting_acquisition_order() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let cfg = BTreeMap::<&str, &str>::new();
        let locker = RepoLocker::new(&cfg, tmp_dir.path().to_path_buf()).unwrap();

        // Lock wlock, then store lock, then wlock again.
        let _wclock1 = locker
            .lock_working_copy(tmp_dir.path().to_path_buf())
            .unwrap();
        let _storelock = locker.lock_store().unwrap();
        let _wclock2 = locker
            .lock_working_copy(tmp_dir.path().to_path_buf())
            .unwrap();
    }

    #[test]
    fn test_try_repo_lock() {
        let tmp_dir = tempfile::tempdir().unwrap();

        let cfg = BTreeMap::<&str, &str>::new();
        let locker1 = RepoLocker::new(&cfg, tmp_dir.path().to_path_buf()).unwrap();
        let locker2 = RepoLocker::new(&cfg, tmp_dir.path().to_path_buf()).unwrap();

        let _lock1 = locker1.try_lock_store().unwrap();
        let _lock2 = locker1.try_lock_store().unwrap();

        // Returns immediately with error.
        assert!(matches!(
            locker2.try_lock_store().unwrap_err(),
            LockError::Contended(_)
        ));
    }

    #[test]
    fn test_lock_legacy_compat() -> Result<()> {
        let tmp = tempfile::tempdir()?;

        let legacy_path = tmp.path().join("foo");

        // legacy path doesn't exist (i.e. rust-only lock mode)
        {
            {
                let _foo_lock = try_lock(tmp.path(), "foo", "some contents".as_bytes())?;
                assert!(legacy_path.exists());
            }

            // clean up legacy file
            assert!(!legacy_path.exists());
        }

        // Legacy path already exists - clean it up.
        {
            File::create(&legacy_path)?;

            {
                let _foo_lock = try_lock(tmp.path(), "foo", "some contents".as_bytes())?;
                assert!(legacy_path.exists());
            }

            // clean up legacy file
            assert!(!legacy_path.exists());
        }

        Ok(())
    }
}
