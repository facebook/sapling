/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::collections::btree_map;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use manifest::FsNodeMetadata;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use slex::Items;
use slex::Work;
use slex::WorkOptions;
use slex::WorkScope;
use slex::WorkShape;
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
    matcher: Arc<dyn Matcher + Sync + Send>,
    store: InnerStore,
}

fn run_worker(
    work: Vec<IterWork>,
    scope: &mut WorkScope<'_, IterWork, IterResult, Error>,
    ctx: &IterContext,
) -> Result<()> {
    if scope.is_canceled() {
        scope.cancel();
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
                }),
                _ => None,
            },
        ),
        ctx.matcher.as_ref(),
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
pub struct DfsCursor<'a> {
    state: State,
    store: &'a InnerStore,
    path: RepoPathBuf,
    link: &'a Link,
    stack: Vec<btree_map::Iter<'a, types::PathComponentBuf, Link>>,
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
        }
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
                    self.state = State::Push;
                    return Step::Success;
                }
                State::Push => {
                    match self.link.as_ref() {
                        // Directories will insert an iterator over their elements in the stack.
                        Ephemeral(links) => {
                            self.stack.push(links.iter());
                            self.state = State::Next;
                        }
                        Durable(durable_entry) => {
                            if let Some(err) = durable_entry.permission_denied_error() {
                                tracing::debug!(path = %self.path, hgid = %durable_entry.hgid, "skipping permission-denied tree in DfsCursor");
                                let mut err = err.clone();
                                err.path = self.path.clone();
                                self.store.record_permission_denied(err);
                                self.state = State::Pop;
                            } else {
                                match durable_entry.materialize_links(self.store, &self.path) {
                                    Err(err) => {
                                        self.state = State::Done;
                                        return Step::Err(err);
                                    }
                                    Ok(links) => self.stack.push(links.iter()),
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
                    match self.stack.last_mut() {
                        // We did not find any iterator with items on the stack.
                        // No more links to iterate over.
                        None => {
                            self.state = State::Done;
                        }
                        Some(last) => {
                            // Take an iterator from the stack and see if they have elements.
                            match last.next() {
                                None => {
                                    // No more elements in this iterator. Remove it from the stack.
                                    self.stack.pop();
                                    self.state = State::Pop;
                                }
                                Some((component, link)) => {
                                    // Found an element. Updating the cursor to point to it.
                                    self.link = link;
                                    self.path.push(component.as_path_component());
                                    self.state = State::Push;
                                    return Step::Success;
                                }
                            }
                        }
                    }
                }
                State::Done => return Step::End,
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use blob::Blob;
    use manifest::Manifest;
    use manifest::PersistOpts;
    use manifest::testutil::*;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::TreeMatcher;
    use storemodel::BoxIterator;
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
    struct RemoteOnlyTreeStore(Arc<TestStore>);

    impl KeyStore for RemoteOnlyTreeStore {
        fn get_content_iter(
            &self,
            fctx: FetchContext,
            keys: Vec<Key>,
        ) -> anyhow::Result<BoxIterator<anyhow::Result<(Key, Blob)>>> {
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
            Arc::new(RemoteOnlyTreeStore(store.clone())),
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
