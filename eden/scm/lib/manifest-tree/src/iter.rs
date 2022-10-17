/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::btree_map;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use async_runtime::RunStream;
use async_runtime::RunStreamOptions;
use futures::channel::mpsc::unbounded;
use futures::stream;
use futures::StreamExt;
use futures_batch::ChunksTimeoutStreamExt;
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
use crate::TreeManifest;

pub struct BfsIter {
    iter: RunStream<Result<(RepoPathBuf, FsNodeMetadata)>>,
    pending: Arc<AtomicU64>,
}

impl BfsIter {
    pub fn new<M: 'static + Matcher + Sync + Send>(tree: &TreeManifest, matcher: M) -> Self {
        let matcher = Arc::new(matcher);
        let store1 = tree.store.clone();
        let store2 = tree.store.clone();
        let (sender, receiver) = unbounded();
        let pending = Arc::new(AtomicU64::new(1));
        sender
            .unbounded_send((RepoPathBuf::new(), tree.root.thread_copy()))
            .expect("unbounded send should always succeed");
        let inner_pending = pending.clone();
        let stream = receiver
            .chunks_timeout(500, Duration::from_millis(1))
            .map(move |chunk| {
                let store = store1.clone();
                async_runtime::spawn_blocking(move || {
                    let keys: Vec<_> = chunk
                        .iter()
                        .filter_map(|(path, link)| {
                            if let Durable(entry) = link.as_ref() {
                                Some(Key::new(path.clone(), entry.hgid.clone()))
                            } else {
                                None
                            }
                        })
                        .collect();
                    let _ = store.prefetch(keys.clone());
                    stream::iter(chunk.into_iter())
                })
            })
            .buffer_unordered(10)
            .map(|r| match r {
                Ok(r) => r,
                Err(e) => {
                    // The child thread paniced.
                    panic!("{:?}", e)
                }
            })
            .flatten()
            .chunks_timeout(200, Duration::from_millis(1))
            .map(move |chunk| {
                let pending = inner_pending.clone();
                let store = store2.clone();
                let matcher = matcher.clone();
                let sender = sender.clone();
                async_runtime::spawn_blocking(move || {
                    let mut results = vec![];
                    'outer: for item in chunk.into_iter() {
                        let (path, link): (RepoPathBuf, Link) = item;
                        let (children, hgid) = match link.as_ref() {
                            Leaf(file_metadata) => {
                                results.push(Ok((path, FsNodeMetadata::File(*file_metadata))));
                                continue;
                            }
                            Ephemeral(children) => (children, None),
                            Durable(entry) => loop {
                                match entry.materialize_links(&store, &path) {
                                    Ok(children) => break (children, Some(entry.hgid)),
                                    Err(e) => {
                                        results.push(Err(e));
                                        continue 'outer;
                                    }
                                };
                            },
                        };
                        for (component, link) in children.iter() {
                            let mut child_path = path.clone();
                            child_path.push(component.as_ref());
                            match link.matches(&matcher, &child_path) {
                                Ok(true) => {
                                    pending.fetch_add(1, Ordering::SeqCst);
                                    sender
                                        .unbounded_send((child_path, link.thread_copy()))
                                        .expect("unbounded_send should always succeed")
                                }
                                Ok(false) => {}
                                Err(e) => {
                                    results.push(Err(e));
                                    continue 'outer;
                                }
                            };
                        }
                        results.push(Ok((path, FsNodeMetadata::Directory(hgid))));
                    }
                    stream::iter(results.into_iter())
                })
            })
            .buffer_unordered(10)
            .map(|r| match r {
                Ok(r) => r,
                Err(e) => {
                    // The child thread paniced.
                    panic!("{:?}", e)
                }
            })
            .flatten();

        BfsIter {
            iter: RunStreamOptions::new().buffer_size(5000).run(stream),
            pending,
        }
    }
}

impl Iterator for BfsIter {
    type Item = Result<(RepoPathBuf, FsNodeMetadata)>;

    fn next(&mut self) -> Option<Self::Item> {
        // If the previous value was 0, then we've already yielded all the values.
        if self.pending.fetch_sub(1, Ordering::SeqCst) == 0 {
            return None;
        }
        self.iter.next()
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
                            match durable_entry.materialize_links(&*self.store, &self.path) {
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
        let tree = TreeManifest::durable(store.clone(), hgid);

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
