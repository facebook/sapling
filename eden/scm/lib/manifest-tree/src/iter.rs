/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Borrow;
use std::collections::btree_map;
use std::mem;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Receiver;
use crossbeam::channel::Sender;
use manifest::FsNodeMetadata;
use pathmatcher::Matcher;
use types::Key;
use types::PathComponentBuf;
use types::RepoPath;
use types::RepoPathBuf;

use crate::link::Durable;
use crate::link::Ephemeral;
use crate::link::Leaf;
use crate::link::Link;
use crate::store::InnerStore;

pub fn bfs_iter<M: 'static + Matcher + Sync + Send>(
    store: InnerStore,
    roots: &[impl Borrow<Link>],
    matcher: M,
) -> Box<dyn Iterator<Item = Result<(RepoPathBuf, FsNodeMetadata)>>> {
    // This channel carries iteration results to the calling code.
    let (result_send, result_recv) =
        crossbeam::channel::unbounded::<Result<(RepoPathBuf, FsNodeMetadata)>>();

    // This channel carries BFS work to the workers threads.
    let (work_send, work_recv) = crossbeam::channel::unbounded::<BfsWork>();

    let worker = BfsWorker {
        work_recv,
        work_send,
        result_send,
        pending: Arc::new(AtomicUsize::new(0)),
        store,
        matcher: Arc::new(matcher),
    };

    // Kick off the search at the root.
    for root in roots {
        worker
            .publish_work(vec![(RepoPathBuf::new(), root.borrow().thread_copy())])
            .unwrap();
    }

    const NUM_BFS_WORKERS: usize = 10;

    for _ in 0..NUM_BFS_WORKERS {
        let worker = worker.clone();
        std::thread::spawn(move || {
            // If the worker returns an error, that signals we should shutdown
            // the whole operation.
            if worker.run().is_err() {
                worker.broadcast_shutdown(NUM_BFS_WORKERS);
            }
        });
    }

    Box::new(result_recv.into_iter())
}

enum BfsWork {
    Walk(Vec<(RepoPathBuf, Link)>),
    Shutdown,
}

#[derive(Clone)]
struct BfsWorker {
    work_recv: Receiver<BfsWork>,
    work_send: Sender<BfsWork>,
    result_send: Sender<Result<(RepoPathBuf, FsNodeMetadata)>>,
    matcher: Arc<dyn Matcher + Sync + Send>,
    store: InnerStore,
    pending: Arc<AtomicUsize>,
}

impl BfsWorker {
    const BATCH_SIZE: usize = 5000;

    fn run(&self) -> Result<()> {
        for work in &self.work_recv {
            let work = match work {
                BfsWork::Walk(work) => work,
                BfsWork::Shutdown => return Ok(()),
            };

            let work_len = work.len();

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

            let _ = self.store.prefetch(keys);

            let mut to_send = Vec::<(RepoPathBuf, Link)>::new();
            for (path, link) in work {
                let (children, hgid) = match link.as_ref() {
                    Leaf(file_metadata) => {
                        self.result_send
                            .send(Ok((path, FsNodeMetadata::File(*file_metadata))))?;
                        continue;
                    }
                    Ephemeral(children) => (children, None),
                    Durable(entry) => match entry.materialize_links(&self.store, &path) {
                        Ok(children) => (children, Some(entry.hgid)),
                        Err(e) => {
                            self.result_send
                                .send(Err(e).context("materialize_links in bfs_iter"))?;
                            continue;
                        }
                    },
                };

                for (component, link) in children.iter() {
                    let mut child_path = path.clone();
                    child_path.push(component.as_ref());
                    match link.matches(&self.matcher, &child_path) {
                        Ok(true) => {
                            to_send.push((child_path, link.thread_copy()));
                            if to_send.len() >= Self::BATCH_SIZE {
                                self.publish_work(mem::take(&mut to_send))?;
                            }
                        }
                        Ok(false) => {}
                        Err(e) => self
                            .result_send
                            .send(Err(e).context("matching in bfs_iter"))?,
                    };
                }

                self.result_send
                    .send(Ok((path, FsNodeMetadata::Directory(hgid))))?;
            }

            self.publish_work(to_send)?;

            if self.pending.fetch_sub(work_len, Ordering::AcqRel) == work_len {
                // If we processed the last work item (i.e. pending has become
                // 0), return an error which will trigger the shutdown of all
                // the worker threads.
                return Err(anyhow!("walk done"));
            }
        }

        unreachable!("worker owns channel send and recv - channel should not disconnect");
    }

    fn publish_work(&self, to_send: Vec<(RepoPathBuf, Link)>) -> Result<()> {
        if to_send.is_empty() {
            return Ok(());
        }

        self.pending.fetch_add(to_send.len(), Ordering::AcqRel);
        Ok(self.work_send.send(BfsWork::Walk(to_send))?)
    }

    fn broadcast_shutdown(&self, num_workers: usize) {
        // I couldn't think of a better way to handle shutdown.
        for _ in 0..num_workers {
            self.work_send.send(BfsWork::Shutdown).unwrap();
        }
    }
}

/// The cursor is a utility for iterating over [`Link`]s. This structure is inteded to be an
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

    use manifest::testutil::*;
    use manifest::Manifest;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::TreeMatcher;
    use types::testutil::*;

    use super::*;
    use crate::prefetch;
    use crate::testutil::*;
    use crate::TreeManifest;

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

        let fetches = store.fetches();

        assert!(fetches.contains(&vec![get_tree_key(&tree1, "")]));
        assert!(fetches.contains(&vec![get_tree_key(&tree1, "a")]));

        assert!(fetches.contains(&vec![get_tree_key(&tree2, "")]));
        assert!(fetches.contains(&vec![get_tree_key(&tree2, "c")]));
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
