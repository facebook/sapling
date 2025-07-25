/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Borrow;
use std::collections::btree_map;
use std::mem;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::bail;
use flume::Receiver;
use flume::Sender;
use flume::WeakSender;
use manifest::FsNodeMetadata;
use once_cell::sync::Lazy;
use pathmatcher::Matcher;
use threadpool::ThreadPool;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::link::Durable;
use crate::link::Ephemeral;
use crate::link::Leaf;
use crate::link::Link;
use crate::store::InnerStore;

/// A thread pool for performing parallel manifest iteration.
/// On drop, in-progress iterations are canceled and threads are cleaned up.
#[derive(Clone)]
struct BfsIterPool {
    #[allow(dead_code)]
    pool: ThreadPool,
    work_send: Sender<BfsWork>,
}

impl BfsIterPool {
    fn new(thread_count: usize) -> Self {
        let pool = ThreadPool::with_name("manifest-bfs-iter".to_string(), thread_count);

        let (work_send, work_recv) = flume::unbounded::<BfsWork>();

        for _ in 0..pool.max_count() {
            let work_recv = work_recv.clone();
            // Give worker a weak sender so the worker doesn't keep the work channel alive
            // indefinitely (and will shut down properly when the strong sender in BfsIterPool is
            // dropped).
            let work_send = work_send.downgrade();
            pool.execute(move || {
                let res = BfsIterPool::run(work_recv, work_send);
                tracing::debug!(?res, "bfs worker exited");
            });
        }

        Self { pool, work_send }
    }
}

static BFS_POOL: Lazy<BfsIterPool> = Lazy::new(|| BfsIterPool::new(num_cpus::get().min(20)));

pub fn bfs_iter<M: 'static + Matcher + Sync + Send>(
    store: InnerStore,
    roots: &[impl Borrow<Link>],
    matcher: M,
) -> Box<dyn Iterator<Item = Result<(RepoPathBuf, FsNodeMetadata)>>> {
    // Pick a sizeable number since each result datum is not very large and we want to keep pipelines full.
    // The important thing is it is less than infinity.
    const RESULT_QUEUE_SIZE: usize = 10_000;

    // This channel carries iteration results to the calling code.
    let (result_send, result_recv) =
        flume::bounded::<Vec<Result<(RepoPathBuf, FsNodeMetadata)>>>(RESULT_QUEUE_SIZE);

    let ctx = BfsContext {
        result_send,
        store,
        matcher: Arc::new(matcher),
    };

    // Kick off the search at the roots.
    BFS_POOL
        .work_send
        .send(BfsWork {
            work: roots
                .iter()
                .map(|root| (RepoPathBuf::new(), root.borrow().thread_copy()))
                .collect(),
            ctx,
        })
        .unwrap();

    Box::new(result_recv.into_iter().flatten())
}

struct BfsWork {
    work: Vec<(RepoPathBuf, Link)>,
    ctx: BfsContext,
}

#[derive(Clone)]
struct BfsContext {
    result_send: Sender<Vec<Result<(RepoPathBuf, FsNodeMetadata)>>>,
    matcher: Arc<dyn Matcher + Sync + Send>,
    store: InnerStore,
}

impl BfsContext {
    fn canceled(&self) -> bool {
        self.result_send.is_disconnected()
    }
}

impl BfsIterPool {
    const BATCH_SIZE: usize = 5000;

    fn run(work_recv: Receiver<BfsWork>, work_send: WeakSender<BfsWork>) -> Result<()> {
        'outer: for BfsWork { work, ctx } in work_recv {
            if ctx.canceled() {
                continue;
            }

            let keys: Vec<_> = work
                .iter()
                .filter_map(|(path, link)| {
                    if let Durable(entry) = link.as_ref() {
                        Some(Key::new(path.clone(), entry.hgid.clone()))
                    } else {
                        None
                    }
                })
                .collect();

            let _ = ctx.store.prefetch(keys);

            let mut work_to_send = Vec::<(RepoPathBuf, Link)>::new();
            let mut results_to_send = Vec::<Result<(RepoPathBuf, FsNodeMetadata)>>::new();
            for (path, link) in work {
                let (children, hgid) = match link.as_ref() {
                    Leaf(_) => {
                        // Publishing file results is handled below before publishing work.
                        continue;
                    }
                    Ephemeral(children) => (children, None),
                    Durable(entry) => match entry.materialize_links(&ctx.store, &path) {
                        Ok(children) => (children, Some(entry.hgid)),
                        Err(e) => {
                            results_to_send.push(Err(e).context("materialize_links in bfs_iter"));
                            continue;
                        }
                    },
                };

                for (component, link) in children.iter() {
                    let mut child_path = path.clone();
                    child_path.push(component.as_path_component());
                    match link.matches(&ctx.matcher, &child_path) {
                        Ok(true) => {
                            if let Leaf(file_metadata) = link.as_ref() {
                                results_to_send
                                    .push(Ok((child_path, FsNodeMetadata::File(*file_metadata))));
                                continue;
                            }

                            work_to_send.push((child_path, link.thread_copy()));
                            if work_to_send.len() >= Self::BATCH_SIZE {
                                if !Self::try_send(
                                    &work_send,
                                    BfsWork {
                                        work: mem::take(&mut work_to_send),
                                        ctx: ctx.clone(),
                                    },
                                )? {
                                    continue 'outer;
                                }
                            }
                        }
                        Ok(false) => {}
                        Err(e) => {
                            results_to_send.push(Err(e).context("matching in bfs_iter"));
                        }
                    };
                }

                results_to_send.push(Ok((path, FsNodeMetadata::Directory(hgid))));
            }

            if !results_to_send.is_empty() {
                if ctx.result_send.send(results_to_send).is_err() {
                    continue 'outer;
                }
            }

            if !Self::try_send(
                &work_send,
                BfsWork {
                    work: work_to_send,
                    ctx,
                },
            )? {
                continue 'outer;
            }
        }

        bail!("work channel disconnected (receiver)")
    }

    /// Publish work into the work queue. Propagates publish errors (indicating pool is shutting down).
    /// Returns false if the walk operation has been canceled.
    fn try_send(work_send: &WeakSender<BfsWork>, work: BfsWork) -> Result<bool> {
        if work.ctx.canceled() {
            return Ok(false);
        }

        if work.work.is_empty() {
            return Ok(true);
        }

        match work_send.upgrade() {
            Some(send) => send.send(work)?,
            None => bail!("work channel disconnected (sender)"),
        }

        Ok(true)
    }
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
    stack: Vec<btree_map::Iter<'a, PathComponentBuf, Link>>,
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
    pub fn path(&self) -> &RepoPath {
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
                            match durable_entry.materialize_links(self.store, &self.path) {
                                Err(err) => {
                                    self.state = State::Done;
                                    return Step::Err(err);
                                }
                                Ok(links) => self.stack.push(links.iter()),
                            }
                            self.state = State::Next;
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

    use manifest::Manifest;
    use manifest::testutil::*;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::TreeMatcher;
    use types::testutil::*;

    use super::*;
    use crate::TreeManifest;
    use crate::prefetch;
    use crate::testutil::*;

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
        let hgid = tree.flush().unwrap();
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
        let tree1_hgid = tree1.flush().unwrap();

        let mut tree2 = TreeManifest::ephemeral(store.clone());
        tree2.insert(repo_path_buf("c/d"), make_meta("2")).unwrap();
        let tree2_hgid = tree2.flush().unwrap();

        prefetch(
            store.clone(),
            &[tree1_hgid, tree2_hgid],
            AlwaysMatcher::new(),
        )
        .unwrap();

        let get_tree_key = |t: &TreeManifest, path: &str| -> Key {
            let path = repo_path_buf(path);
            let hgid = match t.get(&path).unwrap().unwrap() {
                FsNodeMetadata::File(_) => panic!("{path} is a file"),
                FsNodeMetadata::Directory(hgid) => hgid.unwrap(),
            };
            Key::new(path, hgid)
        };

        let fetches = store.prefetches();

        assert!(fetches.contains(&vec![get_tree_key(&tree1, ""), get_tree_key(&tree2, "")]));
        assert!(fetches.contains(&vec![get_tree_key(&tree1, "a"), get_tree_key(&tree2, "c")]));
    }

    #[test]
    fn test_pool_shutdown() {
        let pool = BfsIterPool::new(1);

        let weak_sender = pool.work_send.downgrade();

        drop(pool);

        // Check that the channel is closed.
        assert!(weak_sender.upgrade().is_none());
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
