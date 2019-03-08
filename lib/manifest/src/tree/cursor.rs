// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::collections::btree_map;

use failure::Error;

use types::{PathComponentBuf, RepoPath, RepoPathBuf};

use crate::tree::link::Link;
use crate::tree::store::Store;

/// The cursor is a utility for iterating over [`Link`]s. This structure is inteded to be an
/// implementation detail of other iterating structures. That is why it has some rought edges
/// and a particular use pattern.
/// Because this structure intends to back iterators, it is designed so that `step()` is called on
/// every invocation of `next()`. This should simplify iterator implementations what may want to
/// return the root of the subtree that is being iterated.
pub struct Cursor<'a, S> {
    state: State,
    store: &'a S,
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
    Done,
}

impl<'a, S> Cursor<'a, S> {
    /// Default constructor for Cursor.
    pub fn new(store: &'a S, path: RepoPathBuf, link: &'a Link) -> Self {
        Cursor {
            state: State::Init,
            store,
            path,
            link,
            stack: Vec::new(),
        }
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
}

impl<'a, S: Store> Cursor<'a, S> {
    /// Advances the cursor towards a new [`Link`]. Visiting is done in pre-order.
    /// Errors are an interesting topic but at the time of this writing error only appear in
    /// computing [`DurableEntry`] which cache their failures and calling [`step()`] again retries
    /// to process the same `Link` that it previously failed on.
    pub fn step(&mut self) -> Step {
        // There are two important states phases to this code: State::Push and State::Pop.
        // The Push phase is related to the lazy nature of our durable link. We want to evaluate
        // the children of Links as late as possible. We keep track of the last Link that
        // we visited and push the iterator over the children of that link into our stack.
        // The Pop phase involves calling next on the iterators of the stack until we get a link.
        // Step::Init is also related to the lazy nature of the DurableEntry. The first time
        // we call `step()`, it should "visit" the initial values for path and link.
        // Step::Done means that there are no more links to step over.
        loop {
            match self.state {
                State::Init => {
                    self.state = State::Push;
                    return Step::Success;
                }
                State::Push => {
                    match self.link {
                        // Directories will insert an iterator over their elements in the stack.
                        Link::Ephemeral(links) => self.stack.push(links.iter()),
                        Link::Durable(durable_entry) => {
                            match durable_entry.get_links(self.store, &self.path) {
                                Err(err) => return Step::Err(err),
                                Ok(links) => self.stack.push(links.iter()),
                            }
                        }
                        Link::Leaf(_) => {
                            // Remove the component that we added when this link was added to the
                            // stack.
                            self.path.pop();
                        }
                    };
                    self.state = State::Pop;
                }
                State::Pop => {
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
                                    self.path.pop();
                                    self.stack.pop();
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
