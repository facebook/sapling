/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::collections::btree_map;
use std::mem;
use std::ops::Bound;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use manifest::File;
use manifest::FsNodeMetadata;
use pathmatcher::DirectoryMatch;
use pathmatcher::DynMatcher;
use pathmatcher::Matcher;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use slex::WorkScope;
use slex::WorkShape;
use types::RepoPath;
use types::RepoPathBuf;

use crate::bfs;
use crate::link::Durable;
use crate::link::Ephemeral;
use crate::link::Leaf;
use crate::link::Link;
use crate::store::InnerStore;

type IterWork = (RepoPathBuf, Link, bool);
type IterResult = (RepoPathBuf, FsNodeMetadata);
pub(crate) type BfsItems = Items<IterResult, Error>;

const BATCH_SIZE: usize = 5000;

pub fn bfs_iter<M: 'static + Matcher + Sync + Send>(
    store: InnerStore,
    roots: &[impl Borrow<Link>],
    matcher: M,
) -> BfsItems {
    let ctx = IterContext {
        store,
        matcher: Arc::new(matcher),
    };

    let initial: Vec<IterWork> = roots
        .iter()
        .map(|root| (RepoPathBuf::new(), root.borrow().thread_copy(), false))
        .collect();
    let input: Items<IterWork, Error> = Items::ready(initial);

    Work::run(
        WorkOptions::new()
            .max_workers(bfs::num_workers())
            .inline_items(BATCH_SIZE),
        input,
        WorkShape::batch(move |batch, scope| run_worker(batch, scope, &ctx)),
    )
}

#[derive(Clone)]
struct IterContext {
    matcher: DynMatcher,
    store: InnerStore,
}

fn run_worker(
    work: Result<Vec<IterWork>>,
    scope: &mut WorkScope<'_, IterWork, IterResult, Error>,
    ctx: &IterContext,
) -> Result<()> {
    let work = work?;

    if scope.is_canceled() {
        return Ok(());
    }

    // Batch-prefetch uninitialized durable entries.
    if let Err(e) = bfs::prefetch_trees(
        &ctx.store,
        work.iter().filter_map(
            |(path, link, subtree_matches_everything)| match link.as_ref() {
                Durable(entry) if !entry.is_permission_denied() => Some(bfs::PrefetchTree {
                    path: path.as_repo_path(),
                    entry,
                    subtree_matches_everything: *subtree_matches_everything,
                    matcher_index: 0,
                }),
                _ => None,
            },
        ),
        std::slice::from_ref(&ctx.matcher),
    ) {
        return Err(e).context("prefetch in bfs_iter");
    }

    let mut results_to_send = Vec::<(RepoPathBuf, FsNodeMetadata)>::new();
    for (path, link, subtree_matches_everything) in work {
        let hgid = match &*link {
            Leaf(_) => continue,
            Ephemeral(_) => None,
            Durable(entry) => Some(entry.hgid),
        };

        results_to_send.push((path.clone(), FsNodeMetadata::Directory(hgid)));

        let children = match &*link {
            Leaf(_) => unreachable!(),
            Ephemeral(children) => children,
            Durable(entry) => {
                if let Some(err) = entry.permission_denied_error() {
                    tracing::debug!(path = %path, hgid = %entry.hgid, "skipping permission-denied tree in bfs_iter");
                    let mut err = err.clone();
                    err.path = path.clone();
                    ctx.store.record_permission_denied(err);
                    continue;
                }
                match entry.materialize_links(&ctx.store, &path) {
                    Ok(children) => children,
                    Err(e) => {
                        // A materialization failure makes the traversal incomplete, so stop after
                        // publishing results already produced by this worker.
                        if !results_to_send.is_empty()
                            && !scope.send_result(std::mem::take(&mut results_to_send))
                        {
                            return Ok(());
                        }
                        return Err(e).context("materialize_links in bfs_iter");
                    }
                }
            }
        };

        for (component, link) in children.iter() {
            let mut child_path = path.clone();
            child_path.push(component.as_path_component());
            let directory_match = if subtree_matches_everything {
                Some(DirectoryMatch::Everything)
            } else {
                None
            };

            match &**link {
                Leaf(file_metadata) => {
                    let is_match = if subtree_matches_everything {
                        true
                    } else {
                        ctx.matcher
                            .matches_file(&child_path)
                            .with_context(|| format!("matches_file in bfs_iter for {child_path}"))?
                    };
                    if is_match {
                        results_to_send.push((child_path, FsNodeMetadata::File(*file_metadata)));
                    }
                }
                Durable(_) | Ephemeral(_) => {
                    let directory_match = match directory_match {
                        Some(directory_match) => directory_match,
                        None => ctx
                            .matcher
                            .matches_directory(&child_path)
                            .with_context(|| {
                                format!("matches_directory in bfs_iter for {child_path}")
                            })?,
                    };
                    match directory_match {
                        DirectoryMatch::Nothing => {}
                        DirectoryMatch::ShouldTraverse => {
                            if !scope.submit_work((child_path, link.thread_copy(), false)) {
                                return Ok(());
                            }
                        }
                        DirectoryMatch::Everything => {
                            if !scope.submit_work((child_path, link.thread_copy(), true)) {
                                return Ok(());
                            }
                        }
                    }
                }
            };
        }
    }

    if !results_to_send.is_empty() {
        if !scope.send_result(results_to_send) {
            return Ok(());
        }
    }

    Ok(())
}

/// The cursor is a utility for iterating over [`Link`]s. This structure is intended to be an
/// implementation detail of other iterating structures. That is why it has some rought edges
/// and a particular use pattern.
/// Because this structure intends to back iterators, it is designed so that `step()` is called on
/// every invocation of `next()`. This should simplify iterator implementations what may want to
/// return the root of the subtree that is being iterated.
///
/// With a matcher, the cursor visits only the files the matcher matches and the directories it
/// wants traversed. `scout` lists the directories it will visit next whose trees are not loaded,
/// so a driver can fetch them in batches ahead of the cursor.
pub struct DfsCursor<'a> {
    state: State,
    store: &'a InnerStore,
    path: RepoPathBuf,
    link: &'a Link,
    stack: Vec<btree_map::Range<'a, types::PathComponentBuf, Link>>,
    matcher: Option<DynMatcher>,
}

/// The return type of the [`Cursor::step()`] function.
/// [`Step::Success`] means that the [`Cursor`] has advanced and is now visiting another [`Link`].
/// [`Step::End`] means there are no other [`Link`]s to visit.
/// [`Step::Err`] is returned when a failure is encountered.
#[derive(Debug)]
pub enum Step {
    Success,
    End,
    Err(Error),
}

#[derive(Copy, Clone, Debug, Ord, PartialOrd, Eq, PartialEq, Hash)]
enum State {
    Init,
    Push,
    Pop,
    Next,
    Done,
}

impl<'a> DfsCursor<'a> {
    /// Default constructor for Cursor.
    pub fn new(store: &'a InnerStore, path: RepoPathBuf, link: &'a Link) -> Self {
        DfsCursor {
            state: State::Init,
            store,
            path,
            link,
            stack: Vec::new(),
            matcher: None,
        }
    }

    /// Visit only what `matcher` selects: files it matches and directories it wants traversed.
    pub fn with_matcher(mut self, matcher: DynMatcher) -> Self {
        self.matcher = Some(matcher);
        self
    }

    /// Returns `false` until [`step()`] returns [`Step::End`] and return `true` afterwards.
    pub fn finished(&self) -> bool {
        self.state == State::Done
    }

    /// Returns the [`RepoPath`] for the link that the [`Cursor`] is currently visiting.
    /// Note that after [`Step::End`] is returned from [`step()`], this function will return
    /// the path the cursor was initialed with.
    pub fn path(&self) -> &types::RepoPath {
        self.path.as_repo_path()
    }

    /// Returns the [`Link`] that the [`Cursor`] is currently visiting.
    /// Note that after [`Step::End`] is returned from [`step()`], this function will continue
    /// to return the last link that was visited.
    pub fn link(&self) -> &Link {
        self.link
    }

    /// Will skip all the subtrees under the current [`Link`]. Assuming that the current link is a
    /// directory then this will skip the entire contents (including `evaluation`).
    pub fn skip_subtree(&mut self) {
        match self.state {
            State::Init => {
                self.state = State::Next;
            }
            State::Push => {
                self.state = State::Pop;
            }
            State::Pop => {}
            State::Next => {
                // We don't have any scenario this would be reached.
                panic!("Calling skip_subtree on cursor is not implemented for State::Next");
            }
            State::Done => {}
        }
    }
}

impl<'a> DfsCursor<'a> {
    /// Advances the cursor towards a new [`Link`]. Visiting is done in pre-order.
    /// Errors are an interesting topic. At the time of this writing errors only appear when
    /// computing [`DurableEntry`] (which cache their failures). To protect against potential
    /// infinite loops, when an error is returned from [`step()`], the cursor is transitioned to
    /// State::Done.
    pub fn step(&mut self) -> Step {
        self.step_inner(None)
    }

    /// Position a fresh cursor so that the next [`step()`] continues a walk that stopped at
    /// `after`, the last path it visited: after a file, or inside a directory it had entered.
    /// Trees on the way down are loaded if needed. If a directory of `after` is gone, is no
    /// longer a directory, or cannot be read, the descent stops there and the walk continues
    /// after it at that level. An empty `after` leaves the cursor at the start.
    pub(crate) fn seek_after(&mut self, after: &RepoPath) -> Result<()> {
        debug_assert!(
            self.state == State::Init,
            "seek_after needs a cursor that has not stepped"
        );
        if after.is_empty() {
            return Ok(());
        }
        let mut link = self.link;
        let mut state = State::Next;
        let components: Vec<_> = after.components().collect();
        for (index, component) in components.iter().enumerate() {
            let links = match link.as_ref() {
                Leaf(_) => break,
                Ephemeral(links) => links,
                Durable(entry) => match entry.materialize_links(self.store, &self.path) {
                    Ok(links) => links,
                    Err(err) => {
                        let Some(denied) = entry.permission_denied_error() else {
                            return Err(err);
                        };
                        tracing::debug!(path = %self.path, hgid = %entry.hgid, "skipping permission-denied tree in DfsCursor");
                        let mut denied = denied.clone();
                        denied.path = self.path.clone();
                        self.store.record_permission_denied(denied);
                        // This directory was entered but has no level on the stack.
                        self.path.pop();
                        break;
                    }
                },
            };
            let component = (*component).to_owned();
            self.stack.push(links.range::<types::PathComponentBuf, _>((
                Bound::Excluded(component.clone()),
                Bound::Unbounded,
            )));
            match links.get(&component) {
                Some(child) if !matches!(child.as_ref(), Leaf(_)) => {
                    self.path.push(component.as_path_component());
                    link = child;
                    if index + 1 == components.len() {
                        // `after` is a directory the walk had entered: continue inside it.
                        state = State::Push;
                    }
                }
                _ => break,
            }
        }
        self.link = link;
        self.state = state;
        Ok(())
    }

    /// The directories among the next `runway` the cursor will visit whose trees are not loaded,
    /// in visiting order, found without fetching anything: a copy of the cursor walks ahead
    /// through the trees already loaded and does not look below one that is not.
    pub(crate) fn scout(&self, runway: usize) -> Vec<(RepoPathBuf, Link)> {
        let mut scout = DfsCursor {
            state: self.state,
            store: self.store,
            path: self.path.clone(),
            link: self.link,
            stack: self.stack.clone(),
            matcher: self.matcher.clone(),
        };
        let mut missing = Vec::new();
        let mut dirs = 0;
        while dirs < runway {
            match scout.step_inner(Some(&mut missing)) {
                Step::Success => {
                    if !matches!(scout.link.as_ref(), Leaf(_)) {
                        dirs += 1;
                    }
                }
                Step::End | Step::Err(_) => break,
            }
        }
        missing
    }

    fn step_inner(&mut self, mut scouting: Option<&mut Vec<(RepoPathBuf, Link)>>) -> Step {
        // There are two important states phases to this code: State::Push and State::Next.
        // The Push phase is related to the lazy nature of our durable link. We want to evaluate
        // the children of Links as late as possible. We keep track of the last Link that
        // we visited and push the iterator over the children of that link into our stack. The
        // nuances of this code are related to the separate `push` and `pop` operations on
        // `stack` and `path`.
        // The Next phase involves calling next on the iterators of the stack until we get a link.
        // Step::Init is also related to the lazy nature of the DurableEntry. The first time
        // we call `step()`, it should "visit" the initial values for path and link.
        // Step::Pop manages removing elements from the path.
        // Step::Done means that there are no more links to step over.
        loop {
            match self.state {
                State::Init => {
                    match self.matches(self.link) {
                        Ok(true) => {}
                        Ok(false) => {
                            self.state = State::Done;
                            return Step::End;
                        }
                        Err(err) => {
                            self.state = State::Done;
                            return Step::Err(err);
                        }
                    }
                    self.state = State::Push;
                    return Step::Success;
                }
                State::Push => {
                    let link = self.link;
                    match link.as_ref() {
                        // Directories will insert an iterator over their elements in the stack.
                        Ephemeral(links) => {
                            self.stack
                                .push(links.range::<types::PathComponentBuf, _>(..));
                            self.state = State::Next;
                        }
                        Durable(durable_entry) => {
                            if let Some(err) = durable_entry.permission_denied_error() {
                                tracing::debug!(path = %self.path, hgid = %durable_entry.hgid, "skipping permission-denied tree in DfsCursor");
                                let mut err = err.clone();
                                err.path = self.path.clone();
                                self.store.record_permission_denied(err);
                                self.state = State::Pop;
                            } else if let Some(missing) = scouting
                                .as_deref_mut()
                                .filter(|_| !durable_entry.links_initialized())
                            {
                                // Scouting never fetches: note the directory and skip its subtree.
                                missing.push((self.path.clone(), link.thread_copy()));
                                self.state = State::Pop;
                            } else {
                                match durable_entry.materialize_links(self.store, &self.path) {
                                    Err(err) => {
                                        self.state = State::Done;
                                        return Step::Err(err);
                                    }
                                    Ok(links) => self
                                        .stack
                                        .push(links.range::<types::PathComponentBuf, _>(..)),
                                }
                                self.state = State::Next;
                            }
                        }
                        Leaf(_) => {
                            self.state = State::Pop;
                        }
                    };
                }
                State::Pop => {
                    // There are no subtree elements that are going to be explored so the current
                    // path element should be removed.
                    self.path.pop();
                    self.state = State::Next;
                }
                State::Next => {
                    // We did not find any iterator with items on the stack.
                    // No more links to iterate over.
                    let Some(last) = self.stack.last_mut() else {
                        self.state = State::Done;
                        continue;
                    };
                    // Take an iterator from the stack and see if they have elements.
                    let Some((component, link)) = last.next() else {
                        // No more elements in this iterator. Remove it from the stack.
                        self.stack.pop();
                        self.state = State::Pop;
                        continue;
                    };
                    self.path.push(component.as_path_component());
                    match self.matches(link) {
                        Ok(true) => {
                            // Found an element. Updating the cursor to point to it.
                            self.link = link;
                            self.state = State::Push;
                            return Step::Success;
                        }
                        Ok(false) => {
                            self.path.pop();
                        }
                        Err(err) => {
                            self.state = State::Done;
                            return Step::Err(err);
                        }
                    }
                }
                State::Done => return Step::End,
            }
        }
    }

    /// Whether to visit `link`, found at `self.path`.
    fn matches(&self, link: &Link) -> Result<bool> {
        let Some(matcher) = &self.matcher else {
            return Ok(true);
        };
        match link.as_ref() {
            Leaf(_) => matcher.matches_file(&self.path),
            Ephemeral(_) | Durable(_) => {
                Ok(matcher.matches_directory(&self.path)? != DirectoryMatch::Nothing)
            }
        }
    }
}

/// How many directories a depth-first walk keeps loaded ahead of itself, in visiting order.
const DFS_RUNWAY: usize = 5000;

/// How many files a depth-first walk publishes at a time. The walk batches its own results so a
/// slice can publish everything it found before queuing the next slice, which another worker may
/// run; results are published in the order they are sent.
const DFS_RESULT_BATCH: usize = 128;

/// Work for [`dfs_iter`]: the walk, resuming after the last path it visited, and the trees it
/// asks to have loaded ahead of it.
enum DfsWork {
    Walk(Option<RepoPathBuf>),
    Prefetch(Vec<(RepoPathBuf, Link)>),
}

/// Files under `root` that `matcher` selects, in depth-first order: a directory's entries in name
/// order, with a subdirectory's contents before the entries after it, starting right after
/// `start_after` if given. The walk publishes files and, whenever trees among the next
/// `DFS_RUNWAY` directories it will visit are not loaded, submits them as one batch to its work
/// set and queues the rest of itself behind the batch. With two workers the batch loads while the
/// walk goes on; with one, the batch loads before it. Dropping the items cancels the set.
pub(crate) fn dfs_iter(
    store: InnerStore,
    root: &Link,
    matcher: DynMatcher,
    start_after: Option<RepoPathBuf>,
) -> Items<File, Error> {
    dfs_iter_with_workers(store, root, matcher, start_after, 2)
}

/// [`dfs_iter`] with the worker limit exposed. The set is seeded with the first batch ahead of
/// the walk: a set with a single item would run inline and never promote, and a lone worker,
/// which is what a set gets when the process-wide worker budget is used up, then loads the batch
/// before it walks. A failure to position the first scout is left for the walk to report.
fn dfs_iter_with_workers(
    store: InnerStore,
    root: &Link,
    matcher: DynMatcher,
    start_after: Option<RepoPathBuf>,
    max_workers: usize,
) -> Items<File, Error> {
    let first = positioned(&store, root, &matcher, start_after.as_deref())
        .map(|cursor| cursor.scout(DFS_RUNWAY))
        .unwrap_or_default();
    let root = root.thread_copy();
    let in_flight = Arc::new(AtomicUsize::new(1));
    Work::run(
        WorkOptions::new()
            .max_workers(max_workers)
            .inline_items(0)
            .result_batch_items(1),
        Items::ready(vec![DfsWork::Prefetch(first), DfsWork::Walk(start_after)]),
        WorkShape::batch(move |batch, scope| {
            for work in batch? {
                match work {
                    DfsWork::Walk(resume_after) => {
                        run_dfs_walk(&store, &root, &matcher, resume_after, &in_flight, scope)?
                    }
                    DfsWork::Prefetch(missing) => {
                        prefetch_missing(&store, &missing, &matcher);
                        in_flight.fetch_sub(1, Ordering::AcqRel);
                    }
                }
            }
            Ok(())
        }),
    )
}

/// A cursor over `root` for `matcher`, positioned after `start_after` if given.
fn positioned<'a>(
    store: &'a InnerStore,
    root: &'a Link,
    matcher: &DynMatcher,
    start_after: Option<&RepoPath>,
) -> Result<DfsCursor<'a>> {
    let mut cursor = DfsCursor::new(store, RepoPathBuf::new(), root).with_matcher(matcher.clone());
    if let Some(after) = start_after {
        cursor.seek_after(after)?;
    }
    Ok(cursor)
}

/// One slice of the walk: from after `resume_after` until the walk ends or submits a batch, which
/// queues the next slice behind the batch.
fn run_dfs_walk(
    store: &InnerStore,
    root: &Link,
    matcher: &DynMatcher,
    resume_after: Option<RepoPathBuf>,
    in_flight: &AtomicUsize,
    scope: &mut WorkScope<'_, DfsWork, File, Error>,
) -> Result<()> {
    let mut cursor = positioned(store, root, matcher, resume_after.as_deref())?;
    let mut files = Vec::new();
    // Whether this slice has visited anything, so the cursor's path is where the next resumes.
    let mut visited = false;
    // Directories entered since the last scout, and whether that scout found the runway loaded.
    let mut entered = 0;
    let mut runway_loaded = false;
    loop {
        if scope.is_canceled() {
            return Ok(());
        }
        // Keep loading until the runway is, then look again once the walk has used half of it.
        if in_flight.load(Ordering::Acquire) == 0 && (!runway_loaded || entered >= DFS_RUNWAY / 2) {
            entered = 0;
            let missing = cursor.scout(DFS_RUNWAY);
            runway_loaded = missing.is_empty();
            if !runway_loaded {
                if !scope.send_result(mem::take(&mut files)) {
                    return Ok(());
                }
                in_flight.fetch_add(1, Ordering::AcqRel);
                let resume_after = if visited {
                    Some(cursor.path().to_owned())
                } else {
                    resume_after
                };
                scope.submit_work([DfsWork::Prefetch(missing), DfsWork::Walk(resume_after)]);
                return Ok(());
            }
        }
        match cursor.step() {
            Step::Success => {
                visited = true;
                match cursor.link().as_ref() {
                    Leaf(meta) => {
                        files.push(File::new(cursor.path().to_owned(), *meta));
                        if files.len() >= DFS_RESULT_BATCH
                            && !scope.send_result(mem::take(&mut files))
                        {
                            return Ok(());
                        }
                    }
                    Ephemeral(_) | Durable(_) => entered += 1,
                }
            }
            Step::End => {
                scope.send_result(files);
                return Ok(());
            }
            Step::Err(err) => return Err(err),
        }
    }
}

fn prefetch_missing(store: &InnerStore, missing: &[(RepoPathBuf, Link)], matcher: &DynMatcher) {
    let entries = missing
        .iter()
        .filter_map(|(path, link)| match link.as_ref() {
            Durable(entry) => Some(bfs::PrefetchTree {
                path,
                entry,
                subtree_matches_everything: false,
                matcher_index: 0,
            }),
            _ => None,
        });
    if let Err(err) = bfs::prefetch_trees(store, entries, std::slice::from_ref(matcher)) {
        tracing::debug!(?err, "loading trees ahead of a depth-first walk failed");
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use blob::Blob;
    use manifest::Manifest;
    use manifest::PersistOpts;
    use manifest::testutil::*;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::TreeMatcher;
    use storemodel::ContentFetchItems;
    use storemodel::KeyStore;
    use storemodel::SerializationFormat;
    use storemodel::TreeEntry;
    use storemodel::TreeStore;
    use types::FetchContext;
    use types::HgId;
    use types::Key;
    use types::RepoPath;
    use types::testutil::*;

    use super::*;
    use crate::TreeManifest;
    use crate::prefetch;
    use crate::testutil::*;

    #[derive(Clone)]
    /// Serves trees only through remote fetches, each delayed by the given duration.
    struct RemoteOnlyTreeStore(Arc<TestStore>, Duration);

    impl KeyStore for RemoteOnlyTreeStore {
        fn get_content_iter(
            &self,
            fctx: FetchContext,
            keys: Vec<Key>,
        ) -> anyhow::Result<ContentFetchItems> {
            self.0.get_content_iter(fctx, keys)
        }

        fn get_local_content(&self, path: &RepoPath, hgid: HgId) -> anyhow::Result<Option<Blob>> {
            self.0.get_local_content(path, hgid)
        }

        fn insert_data(
            &self,
            opts: storemodel::InsertOpts,
            path: &RepoPath,
            data: Blob,
        ) -> anyhow::Result<HgId> {
            self.0.insert_data(opts, path, data)
        }

        fn format(&self) -> SerializationFormat {
            self.0.format()
        }

        fn clone_key_store(&self) -> Box<dyn KeyStore> {
            Box::new(self.clone())
        }
    }

    impl TreeStore for RemoteOnlyTreeStore {
        fn get_local_tree(
            &self,
            _path: &RepoPath,
            _hgid: HgId,
        ) -> anyhow::Result<Option<Arc<dyn TreeEntry>>> {
            Ok(None)
        }

        fn get_tree_iter(
            &self,
            fctx: FetchContext,
            keys: Vec<Key>,
        ) -> anyhow::Result<storemodel::TreeFetchItems> {
            std::thread::sleep(self.1);
            self.0.get_tree_iter(fctx, keys)
        }

        fn clone_tree_store(&self) -> Box<dyn TreeStore> {
            Box::new(self.clone())
        }
    }

    #[test]
    fn test_items_empty() {
        let tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        assert!(tree.files(AlwaysMatcher::new()).next().is_none());
        assert_eq!(dirs(&tree, AlwaysMatcher::new()), ["Ephemeral ''"]);
    }

    #[test]
    fn test_items_ephemeral() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();

        assert_eq!(
            tree.files(AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
                make_file("a1/b1/c1/d1", "10"),
            )
        );

        assert_eq!(
            dirs(&tree, AlwaysMatcher::new()),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a2'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a2/b2'",
                "Ephemeral 'a1/b1/c1'"
            ]
        );
    }

    #[test]
    fn test_items_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree = TreeManifest::ephemeral(store.clone());
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        let hgid = Manifest::persist(&mut tree, PersistOpts { parents: &[] }).unwrap();
        let tree = TreeManifest::durable(store, hgid);

        assert_eq!(
            tree.files(AlwaysMatcher::new())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
                make_file("a1/b1/c1/d1", "10"),
            )
        );

        assert_eq!(
            dirs(&tree, AlwaysMatcher::new()),
            [
                "Durable   ''",
                "Durable   'a1'",
                "Durable   'a2'",
                "Durable   'a1/b1'",
                "Durable   'a2/b2'",
                "Durable   'a1/b1/c1'"
            ]
        );
    }

    #[test]
    fn test_items_matcher() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        tree.insert(repo_path_buf("a1/b1/c1/d1"), make_meta("10"))
            .unwrap();
        tree.insert(repo_path_buf("a1/b2"), make_meta("20"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c2"), make_meta("30"))
            .unwrap();
        tree.insert(repo_path_buf("a2/b2/c3"), make_meta("40"))
            .unwrap();
        tree.insert(repo_path_buf("a3/b2/c3"), make_meta("50"))
            .unwrap();

        assert_eq!(
            tree.files(TreeMatcher::from_rules(["a2/b2/**"].iter(), true).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a2/b2/c2", "30"), make_file("a2/b2/c3", "40"))
        );
        assert_eq!(
            tree.files(TreeMatcher::from_rules(["a1/*/c1/**"].iter(), true).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a1/b1/c1/d1", "10"),)
        );
        assert_eq!(
            tree.files(TreeMatcher::from_rules(["**/c3"].iter(), true).unwrap())
                .collect::<Result<Vec<_>>>()
                .unwrap(),
            vec!(make_file("a2/b2/c3", "40"), make_file("a3/b2/c3", "50"))
        );

        // A prefix matcher works as expected.
        assert_eq!(
            dirs(
                &tree,
                TreeMatcher::from_rules(["a1/**"].iter(), true).unwrap()
            ),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a1/b1/c1'"
            ]
        );

        // A suffix matcher is not going to be effective.
        assert_eq!(
            dirs(
                &tree,
                TreeMatcher::from_rules(["**/c2"].iter(), true).unwrap()
            ),
            [
                "Ephemeral ''",
                "Ephemeral 'a1'",
                "Ephemeral 'a2'",
                "Ephemeral 'a3'",
                "Ephemeral 'a1/b1'",
                "Ephemeral 'a2/b2'",
                "Ephemeral 'a3/b2'",
                "Ephemeral 'a1/b1/c1'"
            ]
        );
    }

    #[test]
    fn test_files_finish_on_error_when_collecting_to_vec() {
        let tree = TreeManifest::durable(Arc::new(TestStore::new()), hgid("1"));
        let file_results = tree.files(AlwaysMatcher::new()).collect::<Vec<_>>();
        assert_eq!(file_results.len(), 1);
        assert!(file_results[0].is_err());

        let files_result = tree.files(AlwaysMatcher::new()).collect::<Result<Vec<_>>>();
        assert!(files_result.is_err());
    }

    #[test]
    fn test_multi_root_prefetch() {
        let store = Arc::new(TestStore::new());

        let mut tree1 = TreeManifest::ephemeral(store.clone());
        tree1.insert(repo_path_buf("a/b"), make_meta("1")).unwrap();
        let tree1_hgid = Manifest::persist(&mut tree1, PersistOpts { parents: &[] }).unwrap();

        let mut tree2 = TreeManifest::ephemeral(store.clone());
        tree2.insert(repo_path_buf("c/d"), make_meta("2")).unwrap();
        let tree2_hgid = Manifest::persist(&mut tree2, PersistOpts { parents: &[] }).unwrap();

        prefetch(
            Arc::new(RemoteOnlyTreeStore(store.clone(), Duration::ZERO)),
            &[tree1_hgid, tree2_hgid],
            AlwaysMatcher::new(),
        )
        .unwrap();

        let get_tree_hgid = |t: &TreeManifest, path: &str| -> types::HgId {
            let path = repo_path_buf(path);
            match t.get(&path).unwrap().unwrap() {
                FsNodeMetadata::File(_) => panic!("{path} is a file"),
                FsNodeMetadata::Directory(hgid) => hgid.unwrap(),
            }
        };

        // The iter.rs uses empty paths when collecting keys for get_tree_iter,
        // so we compare by hgid only.
        let fetches: Vec<Vec<types::HgId>> = store
            .fetches()
            .into_iter()
            .map(|batch| {
                let mut hgids: Vec<types::HgId> = batch.into_iter().map(|k| k.hgid).collect();
                hgids.sort();
                hgids
            })
            .collect();

        let check_batch_contains = |expected: Vec<types::HgId>| {
            let mut expected_sorted = expected;
            expected_sorted.sort();
            assert!(
                fetches.contains(&expected_sorted),
                "expected batch {expected_sorted:?} not found in fetches {fetches:?}"
            );
        };

        check_batch_contains(vec![get_tree_hgid(&tree1, ""), get_tree_hgid(&tree2, "")]);
        check_batch_contains(vec![get_tree_hgid(&tree1, "a"), get_tree_hgid(&tree2, "c")]);
    }

    fn dfs_files<M: 'static + Matcher + Sync + Send>(
        tree: &TreeManifest,
        matcher: M,
        start_after: Option<&str>,
    ) -> Vec<File> {
        tree.files_dfs(matcher, start_after.map(repo_path))
            .collect::<Result<Vec<_>>>()
            .unwrap()
    }

    #[test]
    fn test_files_dfs_order() {
        let mut tree = TreeManifest::ephemeral(Arc::new(TestStore::new()));
        for (path, hex) in [
            ("a1/b1/c1/d1", "10"),
            ("a1/b2", "20"),
            ("a2/b2/c2", "30"),
            ("a1/a0", "40"),
        ] {
            tree.insert(repo_path_buf(path), make_meta(hex)).unwrap();
        }

        // Name order within a directory, descending into each subdirectory as it is reached.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), None),
            vec![
                make_file("a1/a0", "40"),
                make_file("a1/b1/c1/d1", "10"),
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
            ]
        );
        assert_eq!(
            dfs_files(
                &tree,
                TreeMatcher::from_rules(["a1/**"].iter(), true).unwrap(),
                None
            ),
            vec![
                make_file("a1/a0", "40"),
                make_file("a1/b1/c1/d1", "10"),
                make_file("a1/b2", "20"),
            ]
        );

        // Resuming continues right after the given file, including out of its directories.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("a1/a0")),
            vec![
                make_file("a1/b1/c1/d1", "10"),
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
            ]
        );
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("a1/b1/c1/d1")),
            vec![make_file("a1/b2", "20"), make_file("a2/b2/c2", "30")]
        );
        // Resuming after a directory the walk had entered continues inside it.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("a1/b1")),
            vec![
                make_file("a1/b1/c1/d1", "10"),
                make_file("a1/b2", "20"),
                make_file("a2/b2/c2", "30"),
            ]
        );
        // A resume point whose directory is gone continues after that directory.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("a1/b1/x/y")),
            vec![make_file("a1/b2", "20"), make_file("a2/b2/c2", "30")]
        );
        // So does one whose directory is now a file.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("a1/b2/x/y")),
            vec![make_file("a2/b2/c2", "30")]
        );
        assert!(dfs_files(&tree, AlwaysMatcher::new(), Some("a2/b2/c2")).is_empty());
    }

    #[test]
    fn test_files_dfs_durable() {
        let store = Arc::new(TestStore::new());
        let mut tree = TreeManifest::ephemeral(store.clone());
        let dirs: Vec<String> = (0..24).map(|i| format!("d{i:02}")).collect();
        for dir in &dirs {
            tree.insert(repo_path_buf(&format!("{dir}/f")), make_meta("10"))
                .unwrap();
        }
        let hgid = Manifest::persist(&mut tree, PersistOpts { parents: &[] }).unwrap();
        // Every tree is a remote fetch that takes a moment, as on a cold cache.
        let remote = Arc::new(RemoteOnlyTreeStore(store.clone(), Duration::from_millis(2)));
        let tree = TreeManifest::durable(remote.clone(), hgid);

        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), None)
                .iter()
                .map(|file| file.path.to_string())
                .collect::<Vec<_>>(),
            dirs.iter()
                .map(|dir| format!("{dir}/f"))
                .collect::<Vec<_>>()
        );
        // The window worker fetched trees in batches while the walk fetched one at a time, so
        // the 25 trees took well under 25 fetches.
        let fetches = store.fetches();
        assert!(
            fetches.len() < 25 && fetches.iter().any(|keys| keys.len() > 1),
            "expected batched tree fetches, got {} fetches",
            fetches.len()
        );

        // Resuming through trees that have to be loaded on the way down.
        assert_eq!(
            dfs_files(&tree, AlwaysMatcher::new(), Some("d21/f"))
                .iter()
                .map(|file| file.path.to_string())
                .collect::<Vec<_>>(),
            ["d22/f", "d23/f"]
        );

        // A set that gets a single worker services the walk and its batches in turn, since the
        // walk queues itself behind each batch it submits: the root, then its children in one
        // fetch.
        let tree = TreeManifest::durable(remote, hgid);
        let fetched = store.fetches().len();
        let files = dfs_iter_with_workers(
            tree.store.clone(),
            &tree.root,
            Arc::new(AlwaysMatcher::new()),
            None,
            1,
        )
        .into_iter()
        .collect::<Result<Vec<_>>>()
        .unwrap();
        assert_eq!(files.len(), dirs.len());
        assert_eq!(store.fetches().len() - fetched, 2);

        // Dropping a partially consumed iterator ends the walk.
        let mut partial = tree.files_dfs(AlwaysMatcher::new(), None);
        assert!(partial.next().is_some());
        drop(partial);
    }

    #[test]
    fn test_dfs_cursor_scouts_ahead_without_fetching() {
        let store = Arc::new(TestStore::new());
        let mut tree = TreeManifest::ephemeral(store.clone());
        for dir in ["d0", "d1", "d2", "d3"] {
            for sub in ["e0", "e1"] {
                tree.insert(repo_path_buf(&format!("{dir}/{sub}/f")), make_meta("10"))
                    .unwrap();
            }
        }
        let hgid = Manifest::persist(&mut tree, PersistOpts { parents: &[] }).unwrap();
        let tree = TreeManifest::durable(store.clone(), hgid);

        let mut cursor = tree.root_cursor();
        assert!(matches!(cursor.step(), Step::Success));
        assert!(matches!(cursor.step(), Step::Success));
        assert_eq!(cursor.path(), repo_path("d0"));

        // Only the root tree is loaded, so the next trees to load are the top-level directories.
        // Scouting lists them in visiting order without fetching anything.
        let fetched = store.fetches().len();
        let roots = cursor.scout(10);
        assert_eq!(
            roots
                .iter()
                .map(|(path, _)| path.to_string())
                .collect::<Vec<_>>(),
            ["d0", "d1", "d2", "d3"]
        );
        assert_eq!(store.fetches().len(), fetched);
        assert_eq!(cursor.scout(2).len(), 2);
    }

    fn dirs<M: 'static + Matcher + Sync + Send>(tree: &TreeManifest, matcher: M) -> Vec<String> {
        tree.dirs(matcher)
            .map(|t| {
                let t = t.unwrap();
                format!(
                    "{:9} '{}'",
                    if t.hgid.is_some() {
                        "Durable"
                    } else {
                        "Ephemeral"
                    },
                    t.path
                )
            })
            .collect::<Vec<_>>()
    }
}
