/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::mem;
use std::sync::Arc;
use std::sync::atomic;
use std::sync::atomic::AtomicUsize;

use anyhow::Result;
use anyhow::anyhow;
use flume::Receiver;
use flume::Sender;
use flume::bounded;
use flume::unbounded;
use manifest::DiffEntry;
use manifest::DiffType;
use manifest::DirDiffEntry;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use progress_model::ActiveProgressBar;
use progress_model::ProgressBar;
use progress_model::Registry;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::DirLink;
use crate::Link;
use crate::THREAD_POOL;
use crate::TreeManifest;
use crate::link::Durable;
use crate::link::Ephemeral;
use crate::link::Leaf;
use crate::store::InnerStore;

#[derive(Debug, Copy, Clone, Eq, PartialEq)]
enum Side {
    Left,
    Right,
}

/// A single item to process as part of the diffing process.
///
/// This may either be a single directory that was only present on one
/// side of the diff, or it may be a pair of directories (with the same
/// path) whose content is different on either side of the diff.
#[derive(Debug, Clone, Eq, PartialEq)]
enum DiffWork {
    // bool is whether this diff was the result of a path conflict
    Single(DirLink, Side, bool),
    Changed(DirLink, DirLink),
}

enum WorkerInput {
    Diff(Vec<DiffWork>),
    Shutdown,
}

impl DiffWork {
    fn process(
        self,
        store: &InnerStore,
        matcher: &dyn Matcher,
        work: &mut Vec<DiffWork>,
        result: ResultSender,
    ) -> Result<()> {
        match self {
            DiffWork::Single(dir, side, path_conflict) => {
                diff_single(dir, side, path_conflict, store, matcher, work, result)
            }
            DiffWork::Changed(left, right) => diff_dirs(left, right, store, matcher, work, result),
        }
    }

    fn path(&self) -> &RepoPath {
        match self {
            DiffWork::Single(d, _, _) => &d.path,
            DiffWork::Changed(d, _) => &d.path,
        }
    }
}

#[derive(Clone)]
pub(crate) enum ResultSender {
    File(Sender<Result<DiffEntry>>),
    Dir(Sender<Result<DirDiffEntry>>),
}

impl ResultSender {
    fn send_file_diff(&self, diff: DiffEntry) -> Result<()> {
        if let Self::File(sender) = self {
            sender.send(Ok(diff))?;
        }
        Ok(())
    }

    fn send_dir_diff(&self, diff: DirDiffEntry) -> Result<()> {
        if let Self::Dir(sender) = self {
            sender.send(Ok(diff))?;
        }
        Ok(())
    }

    fn need_file_diff(&self) -> bool {
        matches!(self, Self::File(_))
    }

    fn need_dir_diff(&self) -> bool {
        matches!(self, Self::Dir(_))
    }

    fn send_error(&self, error: anyhow::Error) -> Result<()> {
        match self {
            Self::File(sender) => sender.send(Err(error))?,
            Self::Dir(sender) => sender.send(Err(error))?,
        }
        Ok(())
    }
}

impl From<Sender<Result<DiffEntry>>> for ResultSender {
    fn from(sender: Sender<Result<DiffEntry>>) -> Self {
        Self::File(sender)
    }
}

impl From<Sender<Result<DirDiffEntry>>> for ResultSender {
    fn from(sender: Sender<Result<DirDiffEntry>>) -> Self {
        Self::Dir(sender)
    }
}

#[derive(Clone)]
struct DiffWorker {
    work_recv: Receiver<WorkerInput>,
    work_send: Sender<WorkerInput>,
    result_send: ResultSender,
    matcher: Arc<dyn Matcher + Sync + Send>,
    store: InnerStore,
    pending: Arc<AtomicUsize>,
    progress_bar: Arc<ProgressBar>,
}

/// A parallel iterator over two trees.
///
/// The iteration is breadth first but in parallel, so different depths can be processed
/// at the same time.
pub(crate) fn diff<T>(
    left: &TreeManifest,
    right: &TreeManifest,
    matcher: Arc<dyn Matcher + Send + Sync>,
) -> Box<dyn Iterator<Item = Result<T>>>
where
    ResultSender: From<Sender<Result<T>>>,
    T: 'static,
{
    let lroot = DirLink::from_root(&left.root).expect("tree root is not a directory");
    let rroot = DirLink::from_root(&right.root).expect("tree root is not a directory");

    // Don't even attempt to perform a diff if these trees are the same.
    if lroot.hgid() == rroot.hgid() && lroot.hgid().is_some() {
        return Box::new(std::iter::empty());
    }

    // Pick a large number since trees are typically small and I don't want to impact perf.
    // The important thing is it is less than infinity.
    const RESULT_QUEUE_SIZE: usize = 100_000;

    // Bound this channel so we don't use up unlimited memory if we are diffing faster
    // than caller is reading results.
    let (result_send, result_recv) = bounded::<Result<T>>(RESULT_QUEUE_SIZE);

    // Use unbounded channel to avoid deadlocks. Memory use should be bounded in practice
    // since workers will block sending results to caller.
    let (work_send, work_recv) = unbounded();

    let store = left.store.clone();

    let progress_bar = ProgressBar::new("diffing manifest", 0, "trees");
    let registry = Registry::main();
    registry.register_progress_bar(&progress_bar);

    let result_send = ResultSender::from(result_send);

    let worker = DiffWorker {
        work_recv,
        work_send,
        result_send,
        pending: Arc::new(AtomicUsize::new(0)),
        store,
        matcher: Arc::new(matcher),
        progress_bar: progress_bar.clone(),
    };

    let thread_count = THREAD_POOL.max_count();

    for _ in 0..thread_count {
        let worker = worker.clone();
        THREAD_POOL.execute(move || {
            // If the worker returns an error, that signals we should shutdown
            // the whole operation.
            if worker.run().is_err() {
                worker.broadcast_shutdown(thread_count);
            }
        });
    }

    worker
        .publish_work(vec![DiffWork::Changed(lroot, rroot)])
        .unwrap();

    Box::new(DiffIter {
        result_recv,
        progress_bar: ProgressBar::push_active(progress_bar, registry),
    })
}

impl DiffWorker {
    // This value is a balance between a large value to get big remote fetch batches and
    // a small value to get more parallelism for CPU intensive tree deserialization and
    // diff operation.
    // In my basic testing, 1000 was faster than 100 and faster than 5000 for a large diff
    // operation.
    const BATCH_SIZE: usize = 1000;

    fn run(&self) -> Result<()> {
        for work in &self.work_recv {
            let work = match work {
                WorkerInput::Diff(work) => work,
                WorkerInput::Shutdown => return Ok(()),
            };

            let work_len = work.len();

            // First prefetch everything for this batch.
            let keys = work.iter().fold(Vec::new(), |mut acc, item| {
                match item {
                    DiffWork::Single(dir, side, _) => {
                        match side {
                            Side::Left => dir.key().map(|key| acc.push(key)),
                            Side::Right => dir.key().map(|key| acc.push(key)),
                        };
                    }
                    DiffWork::Changed(left, right) => {
                        if let Some(key) = left.key() {
                            acc.push(key);
                        }
                        if let Some(key) = right.key() {
                            acc.push(key);
                        }
                    }
                }
                acc
            });
            self.progress_bar.increase_position(keys.len() as u64);
            let _ = self.store.prefetch(keys);

            // Now process diff work, accumulating work for the next tree level.
            let mut to_send = Vec::new();
            for item in work {
                let res = item.process(
                    &self.store,
                    &self.matcher,
                    &mut to_send,
                    self.result_send.clone(),
                );
                if let Err(err) = res {
                    self.result_send.send_error(err)?;
                }

                if to_send.len() >= Self::BATCH_SIZE {
                    self.publish_work(mem::take(&mut to_send))?;
                }
            }
            self.publish_work(to_send)?;

            if self.pending.fetch_sub(work_len, atomic::Ordering::AcqRel) == work_len {
                // If we processed the last work item (i.e. pending has become
                // 0), return an error which will trigger the shutdown of all
                // the worker threads.
                return Err(anyhow!("walk done"));
            }
        }

        unreachable!("worker owns channel send and recv - channel should not disconnect");
    }

    fn publish_work(&self, to_send: Vec<DiffWork>) -> Result<()> {
        if to_send.is_empty() {
            return Ok(());
        }

        self.pending
            .fetch_add(to_send.len(), atomic::Ordering::AcqRel);
        Ok(self.work_send.send(WorkerInput::Diff(to_send))?)
    }

    fn broadcast_shutdown(&self, num_workers: usize) {
        // I couldn't think of a better way to handle shutdown.
        for _ in 0..num_workers {
            self.work_send.send(WorkerInput::Shutdown).unwrap();
        }
    }
}

struct DiffIter<T = DiffEntry> {
    result_recv: Receiver<Result<T>>,
    #[allow(unused)]
    progress_bar: ActiveProgressBar,
}

impl<T> Iterator for DiffIter<T> {
    type Item = Result<T>;

    fn next(&mut self) -> Option<Self::Item> {
        self.result_recv.recv().ok()
    }
}

/// Process a directory that is only present on one side of the diff.
/// Sends diff entries to `result` and adds more work items to `work`.
fn diff_single(
    dir: DirLink,
    side: Side,
    path_conflict: bool,
    store: &InnerStore,
    matcher: &dyn Matcher,
    work: &mut Vec<DiffWork>,
    result: ResultSender,
) -> Result<()> {
    let (files, dirs) = dir.list(store)?;

    if result.need_dir_diff() {
        result.send_dir_diff(DirDiffEntry {
            path: dir.path,
            left: side == Side::Left,
            right: side == Side::Right,
        })?;
    }

    for d in dirs.into_iter() {
        if matcher.matches_directory(&d.path)? != DirectoryMatch::Nothing {
            work.push(DiffWork::Single(d, side, path_conflict));
        }
    }

    for f in files.into_iter() {
        if !path_conflict && f.meta.ignore_unless_conflict {
            continue;
        }

        if result.need_file_diff() && matcher.matches_file(&f.path)? {
            let entry = match side {
                Side::Left => DiffEntry::left(f),
                Side::Right => DiffEntry::right(f),
            };
            result.send_file_diff(entry)?;
        }
    }

    Ok(())
}

/// Diff two directories.
///
/// The directories should correspond to the same path on either side of the
/// diff. Sends diff entries to `result` and adds more work items to `work`.
fn diff_dirs(
    left: DirLink,
    right: DirLink,
    store: &InnerStore,
    matcher: &dyn Matcher,
    work: &mut Vec<DiffWork>,
    result: ResultSender,
) -> Result<()> {
    // Returns whether the parent directory is considered as modified:
    // - Either `l` or `r` is None (added or deleted).
    // - Item type change (file -> dir, or vice-versa).
    let mut add_diffs = |l, r| -> Result<bool> {
        #[cfg(debug_assertions)]
        {
            if let (Some((l_path, _)), Some((r_path, _))) = (l, r) {
                debug_assert_eq!(l_path, r_path);
            }
        }

        let (file_diff, dir_diff) = diff_links(&left.path, l, r);

        let dir_changed = result.need_dir_diff()
            && match (l, r) {
                (Some(..), None) | (None, Some(..)) => true,
                (None, None) => false,
                (Some((_, llink)), Some((_, rlink))) => llink.is_leaf() != rlink.is_leaf(),
            };

        if result.need_file_diff() {
            if let Some(file_diff) = file_diff {
                if matcher.matches_file(&file_diff.path)? {
                    result.send_file_diff(file_diff)?;
                }
            }
        }

        if let Some(dir_diff) = dir_diff {
            if matcher.matches_directory(dir_diff.path())? != DirectoryMatch::Nothing {
                work.push(dir_diff);
            }
        }

        Ok(dir_changed)
    };

    let mut llinks = left.links(store)?.peekable();
    let mut rlinks = right.links(store)?.peekable();
    let mut dir_changed = false;

    loop {
        let item_changed = match (llinks.peek(), rlinks.peek()) {
            (Some((lname, _)), Some((rname, _))) => match lname.cmp(rname) {
                Ordering::Less => add_diffs(llinks.next(), None)?,
                Ordering::Equal => add_diffs(llinks.next(), rlinks.next())?,
                Ordering::Greater => add_diffs(None, rlinks.next())?,
            },
            (Some(_), None) | (None, Some(_)) => add_diffs(llinks.next(), rlinks.next())?,
            (None, None) => break,
        };
        dir_changed = dir_changed || item_changed
    }

    if result.need_dir_diff() && dir_changed {
        result.send_dir_diff(DirDiffEntry {
            path: left.path.clone(),
            left: true,
            right: true,
        })?;
    }

    Ok(())
}

// Diff two items (can be directory, file, or None). If both are present, they must have
// the same name. Returns a file diff and directory diff. There can be no diffs returned,
// just a file diff, just a directory diff, or both.
fn diff_links(
    parent_path: &RepoPath,
    left: Option<(&PathComponentBuf, &Link)>,
    right: Option<(&PathComponentBuf, &Link)>,
) -> (Option<DiffEntry>, Option<DiffWork>) {
    let name = match (left, right) {
        (Some((lname, _)), Some((rname, _))) => {
            assert_eq!(lname, rname);
            lname
        }
        (Some((lname, _)), None) => lname,
        (None, Some((rname, _))) => rname,
        (None, None) => return (None, None),
    };

    let left = left.map(|l| l.1);
    let right = right.map(|l| l.1);

    let path = || -> RepoPathBuf {
        let mut p = parent_path.to_owned();
        p.push(name.as_path_component());
        p
    };

    let (mut dir_diff, mut file_diff) = (None, None);

    match (left.map(|l| l.as_ref()), right.map(|r| r.as_ref())) {
        // Both are files - compare file metadata (including id).
        (Some(Leaf(lmeta)), Some(Leaf(rmeta))) => {
            if lmeta != rmeta {
                file_diff = Some(DiffEntry::new(
                    path(),
                    DiffType::Changed(lmeta.clone(), rmeta.clone()),
                ));
            }
        }
        // Both are directories - short circuit diff if ids match.
        (Some(ldata @ (Durable(_) | Ephemeral(_))), Some(rdata @ (Durable(_) | Ephemeral(_)))) => {
            let mut equal = false;
            if let (Durable(left), Durable(right)) = (ldata, rdata) {
                equal = left.hgid == right.hgid;
            }

            if !equal {
                dir_diff = Some(DiffWork::Changed(
                    DirLink::from_link(left.unwrap(), path()).unwrap(),
                    DirLink::from_link(right.unwrap(), path()).unwrap(),
                ));
            }
        }
        // Differing types.
        _ => {
            let mut single_diff = |link: &Link, side: Side| match link.as_ref() {
                Leaf(meta) => {
                    if meta.ignore_unless_conflict && side == Side::Left && right.is_none() {
                        return;
                    }

                    file_diff = Some(DiffEntry::new(
                        path(),
                        if side == Side::Left {
                            DiffType::LeftOnly(meta.clone())
                        } else {
                            DiffType::RightOnly(meta.clone())
                        },
                    ));
                }
                Durable(_) | Ephemeral(_) => {
                    let dir_link = DirLink::from_link(link, path())
                        .expect("non-leaf node must be a valid directory");

                    // If we don't have a path conflict here, we don't want to mark
                    // unknown files under us as conflicts.
                    let is_conflict = side != Side::Left || right.is_some();
                    dir_diff = Some(DiffWork::Single(dir_link, side, is_conflict));
                }
            };

            if let Some(left) = left {
                single_diff(left, Side::Left);
            }

            if let Some(right) = right {
                single_diff(right, Side::Right);
            }
        }
    };

    (file_diff, dir_diff)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use manifest::DiffType;
    use manifest::File;
    use manifest::FileMetadata;
    use manifest::FileType;
    use manifest::Manifest;
    use manifest::testutil::*;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::TreeMatcher;
    use types::hgid::MF_UNTRACKED_NODE_ID;
    use types::testutil::*;

    use super::*;
    use crate::Link;
    use crate::link::DirLink;
    use crate::testutil::*;

    #[test]
    fn test_diff_entry_from_file() {
        let path = repo_path_buf("foo/bar");
        let meta = make_meta("a");
        let file = File {
            path: path.clone(),
            meta: meta.clone(),
        };

        let left = DiffEntry::left(file.clone());
        let expected = DiffEntry::new(path.clone(), DiffType::LeftOnly(meta.clone()));
        assert_eq!(left, expected);

        let right = DiffEntry::right(file.clone());
        let expected = DiffEntry::new(path.clone(), DiffType::RightOnly(meta.clone()));
        assert_eq!(right, expected);

        let meta2 = make_meta("b");
        let file2 = File {
            path: path.clone(),
            meta: meta2.clone(),
        };

        let changed = DiffEntry::changed(file, file2);
        let expected = DiffEntry::new(path, DiffType::Changed(meta, meta2));
        assert_eq!(changed, expected);
    }

    #[test]
    fn test_diff_single() {
        let store = Arc::new(TestStore::new());
        let tree = make_tree_manifest(store, &[("a", "1"), ("b/f", "2"), ("c", "3"), ("d/f", "4")]);
        let dir = DirLink::from_root(&tree.root).unwrap();
        let (sender, receiver) = unbounded::<Result<DiffEntry>>();
        let mut work = Vec::new();
        let sender = ResultSender::from(sender);

        let matcher = AlwaysMatcher::new();
        diff_single(
            dir,
            Side::Left,
            false,
            &tree.store,
            &matcher,
            &mut work,
            sender,
        )
        .unwrap();

        let expected_entries = vec![
            DiffEntry::new(
                repo_path_buf("a"),
                DiffType::LeftOnly(FileMetadata::new(hgid("1"), FileType::Regular)),
            ),
            DiffEntry::new(
                repo_path_buf("c"),
                DiffType::LeftOnly(FileMetadata::new(hgid("3"), FileType::Regular)),
            ),
        ];
        assert_eq!(
            receiver.into_iter().collect::<Result<Vec<_>>>().unwrap(),
            expected_entries
        );

        let dummy = Link::ephemeral();
        let expected_next = vec![
            DiffWork::Single(
                DirLink::from_link(&dummy, repo_path_buf("b")).unwrap(),
                Side::Left,
                false,
            ),
            DiffWork::Single(
                DirLink::from_link(&dummy, repo_path_buf("d")).unwrap(),
                Side::Left,
                false,
            ),
        ];

        assert_eq!(work, expected_next);
    }

    #[test]
    fn test_diff() {
        let store = Arc::new(TestStore::new());
        let ltree = make_tree_manifest(
            store.clone(),
            &[
                ("changed", "1"),
                ("d1/changed", "1"),
                ("d1/leftonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "1"),
                ("d2/leftonly", "1"),
                ("d2/same", "1"),
                ("leftonly", "1"),
                ("same", "1"),
            ],
        );
        let rtree = make_tree_manifest(
            store,
            &[
                ("changed", "2"),
                ("d1/changed", "2"),
                ("d1/rightonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "2"),
                ("d2/rightonly", "1"),
                ("d2/same", "1"),
                ("rightonly", "1"),
                ("same", "1"),
            ],
        );

        let matcher = AlwaysMatcher::new();
        let diff = ltree.diff(&rtree, matcher).unwrap();
        let entries = diff
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        let expected = vec![
            repo_path_buf("changed"),
            repo_path_buf("leftonly"),
            repo_path_buf("rightonly"),
            repo_path_buf("d1/changed"),
            repo_path_buf("d1/leftonly"),
            repo_path_buf("d1/rightonly"),
            repo_path_buf("d2/changed"),
            repo_path_buf("d2/leftonly"),
            repo_path_buf("d2/rightonly"),
        ];
        assert_eq!(entries, expected);
    }

    #[test]
    fn test_diff_matcher() {
        let store = Arc::new(TestStore::new());
        let ltree = make_tree_manifest(
            store.clone(),
            &[
                ("changed", "1"),
                ("d1/changed", "1"),
                ("d1/leftonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "1"),
                ("d2/leftonly", "1"),
                ("d2/same", "1"),
                ("leftonly", "1"),
                ("same", "1"),
            ],
        );
        let rtree = make_tree_manifest(
            store,
            &[
                ("changed", "2"),
                ("d1/changed", "2"),
                ("d1/rightonly", "1"),
                ("d1/same", "1"),
                ("d2/changed", "2"),
                ("d2/rightonly", "1"),
                ("d2/same", "1"),
                ("rightonly", "1"),
                ("same", "1"),
            ],
        );

        let matcher = TreeMatcher::from_rules(["d1/**"].iter(), true).unwrap();
        let diff = ltree.diff(&rtree, matcher).unwrap();
        let entries = diff
            .collect::<Result<Vec<_>>>()
            .unwrap()
            .into_iter()
            .map(|entry| entry.path)
            .collect::<Vec<_>>();

        let expected = vec![
            repo_path_buf("d1/changed"),
            repo_path_buf("d1/leftonly"),
            repo_path_buf("d1/rightonly"),
        ];
        assert_eq!(entries, expected);
    }

    #[test]
    fn test_diff_generic() {
        let store = Arc::new(TestStore::new());
        let mut left = make_tree_manifest(
            store.clone(),
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")],
        );
        let mut right = make_tree_manifest(
            store,
            &[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")],
        );

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::LeftOnly(make_meta("10"))
                ),
            )
        );
        right
            .insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a1/b2"), make_meta("40"))
            .unwrap();
        left.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .next()
                .is_none()
        );
    }

    #[test]
    fn test_diff_does_not_evaluate_durable_on_hgid_equality() {
        // Leaving the store empty intentionally so that we get a panic if anything is read from it.
        let left = TreeManifest::durable(Arc::new(TestStore::new()), hgid("10"));
        let right = TreeManifest::durable(Arc::new(TestStore::new()), hgid("10"));
        assert!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .next()
                .is_none()
        );

        let right = TreeManifest::durable(Arc::new(TestStore::new()), hgid("20"));
        assert!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .next()
                .unwrap()
                .is_err()
        );
    }

    #[test]
    fn test_diff_one_file_one_directory() {
        let mut left = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        left.insert(repo_path_buf("a1/b1"), make_meta("10"))
            .unwrap();
        left.insert(repo_path_buf("a2"), make_meta("20")).unwrap();

        let mut right = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        right.insert(repo_path_buf("a1"), make_meta("30")).unwrap();
        right
            .insert(repo_path_buf("a2/b2"), make_meta("40"))
            .unwrap();

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1"), DiffType::RightOnly(make_meta("30"))),
                DiffEntry::new(repo_path_buf("a2"), DiffType::LeftOnly(make_meta("20"))),
                DiffEntry::new(repo_path_buf("a1/b1"), DiffType::LeftOnly(make_meta("10"))),
                DiffEntry::new(repo_path_buf("a2/b2"), DiffType::RightOnly(make_meta("40"))),
            )
        );
    }

    #[test]
    fn test_diff_left_empty() {
        let store = Arc::new(TestStore::new());
        let mut left = TreeManifest::ephemeral(store.clone());
        let mut right = make_tree_manifest(
            store,
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a2/b2/c2", "30")],
        );

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
            )
        );

        left.flush().unwrap();
        right.flush().unwrap();

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                DiffEntry::new(repo_path_buf("a1/b2"), DiffType::RightOnly(make_meta("20"))),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
                DiffEntry::new(
                    repo_path_buf("a1/b1/c1/d1"),
                    DiffType::RightOnly(make_meta("10"))
                ),
            )
        );
    }

    #[test]
    fn test_diff_matcher_2() {
        let store = Arc::new(TestStore::new());
        let left = make_tree_manifest(
            store.clone(),
            &[("a1/b1/c1/d1", "10"), ("a1/b2", "20"), ("a3/b1", "40")],
        );
        let right = make_tree_manifest(
            store,
            &[("a1/b2", "40"), ("a2/b2/c2", "30"), ("a3/b1", "40")],
        );

        assert_eq!(
            left.diff(
                &right,
                TreeMatcher::from_rules(["a1/b1/**"].iter(), true).unwrap()
            )
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b1/c1/d1"),
                DiffType::LeftOnly(make_meta("10"))
            ),)
        );
        assert_eq!(
            left.diff(
                &right,
                TreeMatcher::from_rules(["a1/b2"].iter(), true).unwrap()
            )
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a1/b2"),
                DiffType::Changed(make_meta("20"), make_meta("40"))
            ),)
        );
        assert_eq!(
            left.diff(
                &right,
                TreeMatcher::from_rules(["a2/b2/**"].iter(), true).unwrap()
            )
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(DiffEntry::new(
                repo_path_buf("a2/b2/c2"),
                DiffType::RightOnly(make_meta("30"))
            ),)
        );
        assert_eq!(
            left.diff(
                &right,
                TreeMatcher::from_rules(["*/b2/**"].iter(), true).unwrap()
            )
            .unwrap()
            .collect::<Result<Vec<_>>>()
            .unwrap(),
            vec!(
                DiffEntry::new(
                    repo_path_buf("a1/b2"),
                    DiffType::Changed(make_meta("20"), make_meta("40"))
                ),
                DiffEntry::new(
                    repo_path_buf("a2/b2/c2"),
                    DiffType::RightOnly(make_meta("30"))
                ),
            )
        );
        assert!(
            left.diff(
                &right,
                TreeMatcher::from_rules(["a3/**"].iter(), true).unwrap()
            )
            .unwrap()
            .next()
            .is_none()
        );
    }

    #[test]
    fn test_diff_on_sort_order_edge() {
        let store = Arc::new(TestStore::new());
        let left = make_tree_manifest(
            store,
            &[("foo/bar-test/a.txt", "10"), ("foo/bartest/b.txt", "20")],
        );
        let mut right = left.clone();
        right
            .insert(repo_path_buf("foo/bar/c.txt"), make_meta("30"))
            .unwrap();

        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())
                .unwrap()
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec![DiffEntry::new(
                repo_path_buf("foo/bar/c.txt"),
                DiffType::RightOnly(make_meta("30"))
            ),],
        );
    }

    #[test]
    fn test_modified_dirs() {
        let store = Arc::new(TestStore::new());
        let left = make_tree_manifest(
            store.clone(),
            &[
                ("left/a/b", "1"),
                ("unmodified/a/b", "1"),
                ("modified/1/a/b", "1"),
                ("modified/2/a/b", "1"),
                ("modified/3/a/b", "1"),
                ("modified/3/b", "1"),
                ("modified/4/a", "1"),
            ],
        );
        let right = make_tree_manifest(
            store,
            &[
                ("right/a/b", "2"),
                ("unmodified/a/b", "2"),
                ("modified/1/b/a", "2"),
                ("modified/2/a/c", "1"),
                ("modified/3/b", "1"),
                ("modified/4/a/b", "1"),
            ],
        );
        let mut dirs: Vec<String> = left
            .modified_dirs(&right, AlwaysMatcher::new())
            .unwrap()
            .map(|v| dir_diff_entry_to_string(v.unwrap()))
            .collect();
        dirs.sort_unstable();
        assert_eq!(
            dirs,
            [
                "A modified/1/b",
                "A modified/4/a",
                "A right",
                "A right/a",
                "M ",
                "M modified/1",
                "M modified/2/a",
                "M modified/3",
                "M modified/4",
                "R left",
                "R left/a",
                "R modified/1/a",
                "R modified/3/a"
            ]
        );
        // modified has sub-directory changes, but no add/remove or type change.
        // So it should not be considered as modified.
        assert!(!dirs.contains(&"M modified".to_string()));
    }

    fn dir_diff_entry_to_string(entry: DirDiffEntry) -> String {
        let status = match (entry.left, entry.right) {
            (true, true) => "M",
            (true, false) => "R",
            (false, true) => "A",
            (false, false) => "!",
        };
        format!("{} {}", status, entry.path)
    }

    #[test]
    fn test_ignore_unless_conflict() -> Result<()> {
        let store = Arc::new(TestStore::new());

        let untracked_meta = FileMetadata {
            hgid: MF_UNTRACKED_NODE_ID,
            file_type: FileType::Regular,
            ignore_unless_conflict: true,
        };

        let mut left = TreeManifest::ephemeral(store.clone());
        left.insert(repo_path_buf("foo/untracked"), untracked_meta.clone())?;

        // foo/untracked doesn't show in diff since it doesn't conflict

        let right = make_tree_manifest(store.clone(), &[("foo/tracked", "1")]);
        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())?
                .collect::<Result<Vec<_>>>()?,
            vec![DiffEntry::new(
                repo_path_buf("foo/tracked"),
                DiffType::RightOnly(make_meta("1"))
            )],
        );

        // foo/untracked does show in diff since it doesn't conflict

        // "foo/untracked" conflicts with new file "foo/untracked".
        let right = make_tree_manifest(store.clone(), &[("foo/untracked", "1")]);
        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())?
                .collect::<Result<Vec<_>>>()?,
            vec![DiffEntry::new(
                repo_path_buf("foo/untracked"),
                DiffType::Changed(untracked_meta, make_meta("1")),
            )],
        );

        // Parent directory "foo" conflicts with new file "foo".
        let right = make_tree_manifest(store.clone(), &[("foo", "1")]);
        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())?
                .collect::<Result<Vec<_>>>()?,
            vec![
                DiffEntry::new(repo_path_buf("foo"), DiffType::RightOnly(make_meta("1"))),
                DiffEntry::new(
                    repo_path_buf("foo/untracked"),
                    DiffType::LeftOnly(untracked_meta),
                )
            ],
        );

        // File name "foo/untracked" conflicts with new directory "foo/untracked".
        let right = make_tree_manifest(store.clone(), &[("foo/untracked/bar", "1")]);
        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())?
                .collect::<Result<Vec<_>>>()?,
            vec![
                DiffEntry::new(
                    repo_path_buf("foo/untracked"),
                    DiffType::LeftOnly(untracked_meta),
                ),
                DiffEntry::new(
                    repo_path_buf("foo/untracked/bar"),
                    DiffType::RightOnly(make_meta("1"))
                ),
            ],
        );

        // Should not conflict here.
        let right = make_tree_manifest(store, &[]);
        assert_eq!(
            left.diff(&right, AlwaysMatcher::new())?
                .collect::<Result<Vec<_>>>()?,
            vec![],
        );

        Ok(())
    }
}
