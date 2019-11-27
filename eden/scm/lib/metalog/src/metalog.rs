/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use crate::{Error, Result};
use indexedlog::lock::ScopedDirLock;
use indexedlog::log as ilog;
use lazy_static::lazy_static;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::fmt;
use std::path::{Path, PathBuf};
pub use zstore::Id20;
use zstore::Zstore;

/// Key-value metadata storage that can be atomically read and written,
/// and preserves a linear history.
///
/// Suitable to store small metadata. Namely:
/// - The number of different keys are bounded.
///   Ideally the keys is a constant list.
///   Good example: keys are constant, ex. `["bookmarks", "heads"]`.
///   Bad example: keys are dynamic, ex. file names.
/// - The size of values are bounded.
///   Consider using one level of indirection if huge data needs to be
///   stored.
///
/// Values under a same key are delta-ed against their previous versions.  This
/// means the storage is still efficient even if the data is large but changes
/// are small. Unlike traditional mercurial bdiff (used in revlogs), the delta
/// algorithm works fine for binary data.
///
/// The [`MetaLog`] does not specify actual format of `value`s. It's up to the
/// upper layer to define them.
pub struct MetaLog {
    path: PathBuf,

    /// An append-only store - each entry contains a plain Id20
    /// to an object that should be a root.
    log: ilog::Log,

    /// General purposed blob store, keyed by Id20.
    blobs: Zstore,

    /// The Id20 of root that has been written to disk.
    orig_root_id: Id20,

    /// The current (possibly modified) root.
    root: Root,
}

/// Options used by the `commit` API.
#[derive(Default)]
pub struct CommitOptions<'a> {
    pub message: &'a str,
    pub timestamp: u64,

    /// Do not write the new root id to the "roots" log.
    /// This is useful for cross-process transactions.
    pub detached: bool,

    /// How to resolve conflicts.
    ///
    /// The function takes the current MetaLog being written, and another MetaLog
    /// containing the latest change, and an "ancestor" MetaLog that contains data
    /// before the current pending changes.
    /// The function should try to set resolved contents on the current MetaLog and
    /// return `Ok(())` if it is able to resolve everything cleanly.
    pub resolver: Option<Box<dyn FnMut(&mut MetaLog, &MetaLog, &MetaLog) -> Result<()>>>,

    /// Prevent constructing via fields.
    _private: (),
}

impl MetaLog {
    /// Open [`MetaLog`] at the given directory. Create on demand.
    ///
    /// If `root_id` is `None`, the last `root_id` in the log will
    /// be used. Otherwise the specific `root_id` is used.
    pub fn open(path: impl AsRef<Path>, root_id: Option<Id20>) -> Result<MetaLog> {
        let path = path.as_ref();
        let log = Self::ilog_open_options().open(path.join("roots"))?;
        let orig_root_id = match root_id {
            Some(id) => id,
            None => find_last_root_id(&log)?,
        };
        let blobs = Zstore::open(path.join("blobs"))?;
        let root = load_root(&blobs, orig_root_id)?;
        let metalog = MetaLog {
            path: path.to_path_buf(),
            log,
            blobs,
            orig_root_id,
            root,
        };
        Ok(metalog)
    }

    /// List all `root_id`s stored in `path`.
    ///
    /// The oldest `root_id` is returned as the first item.
    pub fn list_roots(path: impl AsRef<Path>) -> Result<Vec<Id20>> {
        let path = path.as_ref();
        let log = Self::ilog_open_options().open(path.join("roots"))?;
        let result = std::iter::once(EMPTY_ROOT_ID.clone())
            .chain(
                log.iter()
                    .map(|e| e.ok().and_then(|e| Id20::from_slice(e).ok()))
                    .take_while(|s| s.is_some())
                    .map(|s| s.unwrap()),
            )
            .collect();
        Ok(result)
    }

    /// Lookup a blob by key.
    pub fn get(&self, name: &str) -> Result<Option<Vec<u8>>> {
        match self.root.map.get(name) {
            Some(&id) => Ok(self.blobs.get(id)?),
            None => Ok(None),
        }
    }

    /// Insert a blob entry with the given name.
    ///
    /// Changes are not flushed to disk. Use `flush` to write them.
    pub fn set(&mut self, name: &str, value: &[u8]) -> Result<Id20> {
        let delta_base_candidates = match self.root.map.get(name) {
            Some(&id) => vec![id],
            None => Vec::new(),
        };
        let new_id = self.blobs.insert(value, &delta_base_candidates)?;
        self.root.map.insert(name.to_string(), new_id);
        Ok(new_id)
    }

    /// Remove an entry.
    ///
    /// Changes are not flushed to disk. Use `flush` to write them.
    pub fn remove(&mut self, name: &str) -> Result<()> {
        self.root.map.remove(name);
        Ok(())
    }

    /// Get names of all keys.
    pub fn keys(&self) -> Vec<&str> {
        self.root.map.keys().map(AsRef::as_ref).collect()
    }

    /// Attempt to write pending changes to disk.
    ///
    /// Return the Id20 that can be passed to `open` for the new (or old) root.
    ///
    /// Raise an error if the on-disk state has changed. The callsite need to
    /// use some lock to protect races (in Mercurial, this can be a repo lock).
    ///
    /// If there are no key-value changes, then `message` and `timestamp` are
    /// discarded.  Otherwise, `message` and `timestamp` will be attached to the
    /// newly created "root" object. This is similar to commit message and date
    /// used by source control.
    pub fn commit(&mut self, options: CommitOptions) -> Result<Id20> {
        if !self.is_dirty() {
            // Nothing changed.
            return Ok(self.orig_root_id);
        }
        let _lock = ScopedDirLock::new(&self.path);
        if self.log.is_changed() && !options.detached {
            // If 'detached' is set, then just write it in a conflict-free way,
            // since the final root object is not committed yet.
            let ancestor = Self::open(&self.path, Some(self.orig_root_id))?;
            let other = Self::open(&self.path, None)?;
            let mut resolver = options.resolver.unwrap_or(Box::new(resolver::fail));
            (resolver)(self, &other, &ancestor)?;
        }
        self.root.message = options.message.to_string();
        self.root.timestamp = options.timestamp;
        let bytes = mincode::serialize(&self.root)?;
        let id = self.blobs.insert(&bytes, &vec![self.orig_root_id])?;
        self.blobs.flush()?;
        if !options.detached {
            self.log.append(id.as_ref())?;
            self.log.sync()?;
            self.orig_root_id = id;
        }
        Ok(id)
    }

    /// Test if there are uncommitted changes.
    pub fn is_dirty(&self) -> bool {
        let bytes =
            mincode::serialize(&self.root).expect("in-memory mincode::serialize should success");
        zstore::sha1(&bytes) != self.orig_root_id
    }

    /// Why this change was made.
    pub fn message(&self) -> &str {
        &self.root.message
    }

    /// When this change was made, in seconds since epch.
    pub fn timestamp(&self) -> u64 {
        self.root.timestamp
    }

    /// Generate an error.
    pub fn error(&self, message: impl fmt::Display) -> Error {
        Error(format!("{:?}: {}", &self.path, message))
    }

    fn ilog_open_options() -> ilog::OpenOptions {
        ilog::OpenOptions::new()
            .index("reverse", |_| -> Vec<_> {
                // Reverse index so we can find the last entries quickly.
                vec![ilog::IndexOutput::Owned(
                    INDEX_REVERSE_KEY.to_vec().into_boxed_slice(),
                )]
            })
            .create(true)
    }
}

fn find_last_root_id(log: &ilog::Log) -> Result<Id20> {
    for entry in log.lookup(INDEX_REVERSE, INDEX_REVERSE_KEY)? {
        // The linked list in the index is in the reversed order.
        // So the first entry contains the last root id.
        return Ok(Id20::from_slice(entry?)?);
    }
    Ok(EMPTY_ROOT_ID.clone())
}

fn load_root(blobs: &Zstore, id: Id20) -> Result<Root> {
    let root = match blobs.get(id)? {
        Some(bytes) => mincode::deserialize(&bytes)?,
        None => EMPTY_ROOT.clone(),
    };
    Ok(root)
}

const INDEX_REVERSE: usize = 0;
const INDEX_REVERSE_KEY: &[u8] = b"r";

lazy_static! {
    static ref EMPTY_ROOT: Root = Root::default();
    static ref EMPTY_ROOT_ID: Id20 = zstore::sha1(
        &mincode::serialize(EMPTY_ROOT.deref()).expect("serialize EMPTY_ROOT should not fail")
    );
}

#[derive(Serialize, Deserialize, Default, Clone, Debug)]
struct Root {
    map: BTreeMap<String, Id20>,
    timestamp: u64,
    message: String,
}

/// Predefined conflict resolutions.
pub mod resolver {
    use super::MetaLog;
    use crate::Result;
    use std::collections::BTreeSet;

    /// Fail the merge unconditionally on any kind of conflicts.
    pub fn fail(this: &mut MetaLog, other: &MetaLog, ancestor: &MetaLog) -> Result<()> {
        let mut conflicts = BTreeSet::new();
        for key in other.keys().iter().chain(this.keys().iter()) {
            let ancestor_id = ancestor.root.map.get(&key.to_string());
            let other_id = other.root.map.get(&key.to_string());
            let this_id = this.root.map.get(&key.to_string());
            let changed_description = match (
                ancestor_id == this_id,
                ancestor_id == other_id,
                this_id == other_id,
            ) {
                (false, false, false) => "both changed, diverged",
                (false, false, true) => "both changed, same",
                (true, false, _) => "this changed",
                (false, true, _) => "other changed",
                (true, true, _) => continue,
            };
            conflicts.insert(format!("  {}: {}", key, changed_description));
        }
        let message = if conflicts.is_empty() {
            "conflict detected".to_string()
        } else {
            format!(
                "conflict detected:\n{}",
                conflicts.into_iter().collect::<Vec<_>>().join("\n")
            )
        };
        Err(this.error(message))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::quickcheck;
    use tempfile::TempDir;

    #[test]
    fn test_root_id() {
        let dir = TempDir::new().unwrap();
        let mut metalog = MetaLog::open(&dir, None).unwrap();
        metalog.set("foo", b"bar").unwrap();
        assert_eq!(MetaLog::list_roots(&dir).unwrap().len(), 1);

        metalog.commit(commit_opt("commit 1", 11)).unwrap();
        assert_eq!(MetaLog::list_roots(&dir).unwrap().len(), 2);

        // no-op change
        metalog.set("foo", b"bar3").unwrap();
        metalog.set("foo", b"bar").unwrap();
        metalog.commit(commit_opt("commit 1-noop", 111)).unwrap();

        metalog.set("foo", b"bar2").unwrap();
        metalog.commit(commit_opt("commit 2", 22)).unwrap();
        let root_ids = MetaLog::list_roots(&dir).unwrap();
        assert_eq!(root_ids.len(), 3);

        let metalog = MetaLog::open(&dir, Some(root_ids[0])).unwrap();
        assert!(metalog.keys().is_empty());
        assert_eq!(metalog.message(), "");
        assert_eq!(metalog.timestamp(), 0);

        let metalog = MetaLog::open(&dir, Some(root_ids[1])).unwrap();
        assert_eq!(metalog.get("foo").unwrap().unwrap(), b"bar");
        assert_eq!(metalog.message(), "commit 1");
        assert_eq!(metalog.timestamp(), 11);

        let metalog = MetaLog::open(&dir, Some(root_ids[2])).unwrap();
        assert_eq!(metalog.get("foo").unwrap().unwrap(), b"bar2");
        assert_eq!(metalog.message(), "commit 2");
        assert_eq!(metalog.timestamp(), 22);
    }

    #[test]
    fn test_default_resolver() {
        let dir = TempDir::new().unwrap();
        let mut metalog = MetaLog::open(&dir, None).unwrap();
        metalog.set("00", b"0").unwrap();
        metalog.set("10", b"0").unwrap();
        metalog.set("01", b"0").unwrap();
        metalog.set("11a", b"0").unwrap();
        metalog.set("11b", b"0").unwrap();
        metalog.commit(commit_opt("commit 0", 0)).unwrap();

        let mut metalog1 = MetaLog::open(&dir, None).unwrap();
        let mut metalog2 = MetaLog::open(&dir, None).unwrap();
        metalog1.set("10", b"1").unwrap();
        metalog1.set("11a", b"1").unwrap();
        metalog1.set("11b", b"1").unwrap();
        metalog2.set("01", b"1").unwrap();
        metalog2.set("11a", b"1").unwrap();
        metalog2.set("11b", b"2").unwrap();

        metalog1.commit(commit_opt("commit 1", 1)).unwrap();
        let err = metalog2
            .commit(commit_opt("commit 2", 2))
            .unwrap_err()
            .to_string()
            .replace(&format!("{:?}", dir.path()), "<path>");
        assert_eq!(
            err,
            r#"<path>: conflict detected:
  01: other changed
  10: this changed
  11a: both changed, same
  11b: both changed, diverged"#
        );
    }

    #[test]
    fn test_custom_resolver() {
        let dir = TempDir::new().unwrap();
        let mut metalog = MetaLog::open(&dir, None).unwrap();
        metalog.set("a", b"0").unwrap();
        metalog.commit(commit_opt("commit 0", 0)).unwrap();

        let mut metalog1 = MetaLog::open(&dir, None).unwrap();
        let mut metalog2 = MetaLog::open(&dir, None).unwrap();
        metalog1.set("a", b"1").unwrap();
        metalog2.set("a", b"2").unwrap();
        metalog1.commit(commit_opt("commit 1", 1)).unwrap();

        let mut opts = commit_opt("commit 2", 2);
        opts.resolver = Some(Box::new(
            |this: &mut MetaLog, other: &MetaLog, _ancestor: &MetaLog| -> Result<()> {
                // Concatenate both sides.
                let mut v1 = this.get("a").unwrap().unwrap();
                let mut v2 = other.get("a").unwrap().unwrap();
                v1.append(&mut v2);
                this.set("a", &v1).unwrap();
                // Also try to write an extra key.
                this.set("c", b"c").unwrap();
                Ok(())
            },
        ));
        metalog2.commit(opts).unwrap();

        // Check the merged content.
        let metalog3 = MetaLog::open(&dir, None).unwrap();
        assert_eq!(metalog3.message(), "commit 2");
        assert_eq!(metalog3.get("a").unwrap().unwrap(), b"21");
        assert_eq!(metalog3.get("c").unwrap().unwrap(), b"c");
    }

    quickcheck! {
        fn test_random_round_trips(map: BTreeMap<String, (Vec<u8>, Vec<u8>)>) -> bool {
            test_round_trips(map);
            true
        }
    }

    fn test_round_trips(map: BTreeMap<String, (Vec<u8>, Vec<u8>)>) {
        let dir = TempDir::new().unwrap();
        let mut metalog = MetaLog::open(&dir, None).unwrap();

        for (k, (v1, v2)) in map.iter() {
            metalog.set(k, v1).unwrap();
            assert_eq!(metalog.get(k).unwrap(), Some(v1.clone()));
            metalog.set(k, v2).unwrap();
            assert_eq!(metalog.get(k).unwrap(), Some(v2.clone()));
        }
        let root_id1 = metalog.commit(commit_opt("", 0)).unwrap();

        for (k, (_v1, v2)) in map.iter() {
            metalog.remove(k).unwrap();
            assert_eq!(metalog.get(k).unwrap(), None);
            metalog.set(k, v2).unwrap();
            assert_eq!(metalog.get(k).unwrap(), Some(v2.clone()));
        }
        let root_id2 = metalog.commit(commit_opt("", 0)).unwrap();
        assert_eq!(root_id1, root_id2);

        metalog.commit(commit_opt("", 0)).unwrap();
        let metalog = MetaLog::open(&dir, None).unwrap();
        for (k, (_v1, v2)) in map.iter() {
            assert_eq!(metalog.get(k).unwrap(), Some(v2.clone()));
        }
        assert_eq!(metalog.keys(), map.keys().collect::<Vec<_>>());
    }

    fn commit_opt(message: &str, timestamp: u64) -> CommitOptions {
        let mut opts = CommitOptions::default();
        opts.message = message;
        opts.timestamp = timestamp;
        opts
    }
}
