// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Virtual File System (Vfs), provides API to walk through files and read their content.
//! The main usage is to creat a Vfs based on Manifest.

use std::collections::VecDeque;
use std::mem;

use futures::{Async, Future, Poll, Stream};

use mercurial_types::manifest::Content;
use mononoke_types::MPathElement;

use errors::*;

const MAX_STEPS: usize = 64;

/// Represents a node within a Virtual File System. It can be either a Directory or a File
#[derive(Debug, Clone)]
pub enum VfsNode<TDir, TFile> {
    /// Directory within a Vfs
    Dir(TDir),
    /// File within a Vfs
    File(TFile),
}

/// Directory within a Vfs
pub trait VfsDir
where
    Self: Sized + Send + 'static + Clone,
{
    /// Associated type for node that is a File
    type TFile: VfsFile;

    /// Returns the content of the directory
    fn read(&self) -> Vec<&MPathElement>;

    /// Steps one level through Vfs following a given MPathElement and returns the corresponding
    /// VfsNode. Returns None if no such MPathElement exists in the directory
    fn step(&self, path_element: &MPathElement) -> Option<VfsNode<Self, Self::TFile>>;

    /// Wrap self into a VfsNode
    fn into_node(self) -> VfsNode<Self, Self::TFile> {
        VfsNode::Dir(self)
    }
}

/// File within a Vfs
pub trait VfsFile
where
    Self: Sized + Send + 'static + Clone,
{
    /// Associated type for node that is a Dir
    type TDir: VfsDir;

    /// Returns a future with the content of the file
    fn read(&self) -> Box<Future<Item = Content, Error = Error> + Send>;

    /// Returns directory that contains this file
    fn parent_dir(&self) -> Self::TDir;

    /// Wrap self into a VfsNode
    fn into_node(self) -> VfsNode<Self::TDir, Self> {
        VfsNode::File(self)
    }
}

/// Structure for walking the Vfs
pub struct VfsWalker<TDir, TFile> {
    current_node: VfsNode<TDir, TFile>,
    remainder: VecDeque<MPathElement>,
    max_steps: usize,
    steps: usize,
}

impl<TDir, TFile> VfsWalker<TDir, TFile>
where
    TDir: VfsDir<TFile = TFile> + 'static + Send,
    TFile: VfsFile<TDir = TDir> + 'static + Send,
{
    /// Creates a Stream of VfsNode that is encountered on the provided path from the given root.
    /// Will return an error when the path was not found in the Vfs or when maximum number of steps
    /// was reached while resolving the path
    pub fn new<P>(current_node: VfsNode<TDir, TFile>, path: P) -> Self
    where
        P: IntoIterator<Item = MPathElement>,
    {
        Self::with_max_steps(current_node, path, MAX_STEPS)
    }

    /// Similar to `VfsWalker::new`, but you can provide the maximum number of steps for the walker
    pub fn with_max_steps<P>(current_node: VfsNode<TDir, TFile>, path: P, max_steps: usize) -> Self
    where
        P: IntoIterator<Item = MPathElement>,
    {
        VfsWalker {
            current_node,
            remainder: path.into_iter().collect(),
            max_steps,
            steps: 0,
        }
    }

    /// Consumes this Stream and returns a Future that resolves into the last VfsNode on the path
    pub fn walk(self) -> Box<Future<Item = VfsNode<TDir, TFile>, Error = Error> + Send> {
        let node = self.current_node.clone();
        Box::new(self.fold(node, |_, node| Ok::<_, Error>(node)))
    }
}

impl<TDir, TFile> Stream for VfsWalker<TDir, TFile>
where
    TDir: VfsDir<TFile = TFile> + Clone,
    TFile: VfsFile<TDir = TDir> + Clone,
{
    type Item = VfsNode<TDir, TFile>;
    type Error = Error;

    fn poll(&mut self) -> Poll<Option<Self::Item>, Error> {
        if self.steps == 0 {
            self.steps += 1;
            return Ok(Async::Ready(Some(self.current_node.clone())));
        }

        if self.steps > self.max_steps && !self.remainder.is_empty() {
            let remainder = mem::replace(&mut self.remainder, VecDeque::new());
            bail_err!(ErrorKind::MaximumStepReached(
                format!(
                    "Reached a maximum of {} steps during a walk",
                    self.max_steps
                ),
                remainder,
            ));
        }

        match self.remainder.pop_front() {
            None => Ok(Async::Ready(None)),
            Some(path_element) => {
                self.current_node = match self.current_node {
                    VfsNode::Dir(ref dir) => match dir.step(&path_element) {
                        None => {
                            let mut remainder = mem::replace(&mut self.remainder, VecDeque::new());
                            remainder.push_front(path_element);
                            bail_err!(ErrorKind::PathDoesNotExist(
                                "Encountered a non existing MPath during a walk on Vfs".into(),
                                remainder,
                            ));
                        }
                        Some(node) => node,
                    },
                    VfsNode::File(_) => bail_err!(ErrorKind::NotImplemented(
                        "Walking through Symlinks is not implemented yet".into(),
                    )),
                };
                self.steps += 1;
                Ok(Async::Ready(Some(self.current_node.clone())))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test::*;

    use boxfnonce::BoxFnOnce;
    use itertools::assert_equal;

    use mercurial_types::MPath;

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MockVfsDir(VecDeque<MPathElement>, Option<MockVfsFile>);

    impl VfsDir for MockVfsDir {
        type TFile = MockVfsFile;

        fn read(&self) -> Vec<&MPathElement> {
            match self.0.front() {
                Some(ref el) => vec![el],
                None => vec![],
            }
        }

        fn step(&self, path_element: &MPathElement) -> Option<VfsNode<Self, Self::TFile>> {
            let mut path = self.0.clone();
            if path.front() == Some(path_element) {
                path.pop_front();
                if path.is_empty() {
                    match self.1 {
                        None => None,
                        Some(ref file) => Some(file.clone().into_node()),
                    }
                } else {
                    Some((MockVfsDir(path, self.1.clone())).into_node())
                }
            } else {
                None
            }
        }
    }

    #[derive(Clone, Debug, PartialEq, Eq)]
    struct MockVfsFile;

    impl VfsFile for MockVfsFile {
        type TDir = MockVfsDir;

        fn read(&self) -> Box<Future<Item = Content, Error = Error> + Send> {
            unimplemented!();
        }
        fn parent_dir(&self) -> Self::TDir {
            unimplemented!();
        }
    }

    fn make_node(
        path: &'static str,
        file: Option<MockVfsFile>,
    ) -> VfsNode<MockVfsDir, MockVfsFile> {
        let path = MPath::new(path).unwrap().into_iter().collect();
        (MockVfsDir(path, file)).into_node()
    }

    fn cmp_paths<'a, Els, P>(value: Els, expected: P)
    where
        Els: IntoIterator<Item = &'a MPathElement>,
        P: AsRef<[u8]>,
    {
        let value = value.into_iter();
        let expected = MPath::new(expected).unwrap();
        assert_equal(value, (&expected).into_iter());
    }

    type Checker<TDir, TFile> = BoxFnOnce<'static, (Result<VfsNode<TDir, TFile>>,)>;

    fn check_dir<Els, TDir, TFile>(expected: Els) -> Checker<TDir, TFile>
    where
        Els: 'static + IntoIterator<Item = &'static str>,
        TDir: VfsDir,
    {
        BoxFnOnce::from(move |result: Result<VfsNode<TDir, TFile>>| {
            let expected = expected.into_iter().map(pel);
            match result.unwrap() {
                VfsNode::Dir(dir) => assert_equal(dir.read().into_iter().cloned(), expected),
                _ => panic!("expected dir"),
            }
        })
    }

    fn check_file<TDir, TFile>() -> Checker<TDir, TFile>
    where
        TFile: VfsFile,
    {
        BoxFnOnce::from(
            move |result: Result<VfsNode<TDir, TFile>>| match result.unwrap() {
                VfsNode::File(_) => (),
                _ => panic!("expected dir"),
            },
        )
    }

    fn check_not_exists<TDir, TFile>(expected: &'static str) -> Checker<TDir, TFile> {
        BoxFnOnce::from(move |result: Result<VfsNode<TDir, TFile>>| {
            let expected = MPath::new(expected).unwrap().into_iter();
            match result {
                Err(error) => match err_downcast!(error, err: ErrorKind => err) {
                    Ok(ErrorKind::PathDoesNotExist(_, r)) => {
                        assert_equal(r, expected);
                    }
                    Ok(error) => panic!("unexpected ErrorKind error: {:?}", error),
                    Err(error) => panic!("unexpected other error: {:?}", error),
                },
                Ok(_) => panic!("unexpected success"),
            }
        })
    }

    #[test]
    fn test_walker_stream() {
        let node = make_node("a/b/c/d", None);
        let result = VfsWalker::new(node, MPath::new("a/b/c").unwrap())
            .map(|node| match node {
                VfsNode::Dir(dir) => dir.read().into_iter().cloned().collect::<Vec<_>>(),
                _ => panic!("expected dir"),
            })
            .collect()
            .wait()
            .unwrap();
        assert_eq!(
            result,
            vec![
                vec![pel("a")],
                vec![pel("b")],
                vec![pel("c")],
                vec![pel("d")],
            ]
        );
    }

    #[test]
    fn test_walk_no_files() {
        let node = make_node("a/b/c", None);

        for (path, step_limit, checker) in vec![
            (None, 0, check_dir(vec!["a"])),
            (Some("a"), 1, check_dir(vec!["b"])),
            (Some("a/b"), 2, check_dir(vec!["c"])),
            (Some("a/b/c"), 3, check_not_exists("c")),
            (Some("d/e"), MAX_STEPS, check_not_exists("d/e")),
        ] {
            let path = path.map(|p| MPath::new(p).unwrap());
            checker.call(
                VfsWalker::with_max_steps(node.clone(), MPath::into_iter_opt(path), step_limit)
                    .walk()
                    .wait(),
            );
        }
    }

    #[test]
    fn test_walk_limited_by_steps() {
        let node = make_node("a/b/c", None);

        for (path, step_limit, expected_remainder, checker) in vec![
            ("a", 0, "a", check_dir(vec!["b"])),
            ("a/b", 0, "a/b", check_dir(vec!["c"])),
            ("a/b", 1, "b", check_dir(vec!["c"])),
            ("a/b/c", 1, "b/c", check_not_exists("c")),
        ] {
            let mut walk_result =
                VfsWalker::with_max_steps(node.clone(), MPath::new(path).unwrap(), step_limit)
                    .wait()
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>();
            assert_eq!(walk_result.len(), 2, "expected a walk that ends with error");
            let last = walk_result.pop().unwrap().unwrap();
            let err = walk_result.pop().unwrap().unwrap_err();

            match err_downcast!(err, err: ErrorKind => err) {
                Ok(ErrorKind::MaximumStepReached(_, r)) => {
                    cmp_paths(&r, expected_remainder);
                    checker.call(VfsWalker::new(last, r).walk().wait());
                }
                Ok(error) => panic!("unexpected ErrorKind error: {:?}", error),
                Err(error) => panic!("unexpected other error: {:?}", error),
            }
        }
    }

    #[test]
    fn test_walk_with_file() {
        let node = make_node("a/b/c", Some(MockVfsFile));

        let check_not_implemented = BoxFnOnce::from(|result: Result<_>| match result {
            Err(error) => match err_downcast!(error, err: ErrorKind => err) {
                Ok(ErrorKind::NotImplemented(_)) => (),
                Ok(error) => panic!("unexpected ErrorKind error: {:?}", error),
                Err(error) => panic!("unexpected other error: {:?}", error),
            },
            Ok(_) => panic!("unexpected success"),
        });

        for (path, step_limit, checker) in vec![
            ("a/b/c", 3, check_file()),
            ("a/b/c/d", 4, check_not_implemented),
        ] {
            checker.call(
                VfsWalker::with_max_steps(node.clone(), MPath::new(path).unwrap(), step_limit)
                    .walk()
                    .wait(),
            );
        }
    }
}
