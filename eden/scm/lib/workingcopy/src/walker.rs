/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs;
use std::fs::DirEntry;
use std::fs::Metadata;
use std::io;
use std::num::NonZeroU8;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::unbounded;
use crossbeam::channel::Receiver;
use crossbeam::channel::RecvError;
use crossbeam::channel::RecvTimeoutError;
use crossbeam::channel::Sender;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use thiserror::Error;
use types::path::ParseError;
use types::RepoPath;
use types::RepoPathBuf;

#[derive(Error, Debug)]
pub enum WalkError {
    #[error("invalid file name encoding '{0}'")]
    FsUtf8Error(String),
    #[error("IO error at '{0}': {1}")]
    IOError(RepoPathBuf, #[source] io::Error),
    #[error("path error at '{0}': {1}")]
    RepoPathError(String, #[source] ParseError),
    #[error("invalid file type at '{0}'")]
    InvalidFileType(RepoPathBuf),
    #[error("invalid mtime for '{0}': {1}")]
    InvalidMTime(RepoPathBuf, #[source] anyhow::Error),
    #[error("channel disconnected")]
    ChannelDisconnected(#[source] RecvTimeoutError),
    #[error("channel recv error")]
    ChannelRecvError(#[source] RecvError),
}

impl WalkError {
    pub fn filename(&self) -> String {
        match self {
            WalkError::FsUtf8Error(path) => path.to_string(),
            WalkError::IOError(path, _) => path.to_string(),
            WalkError::RepoPathError(path, _) => path.to_string(),
            WalkError::InvalidFileType(path) => path.to_string(),
            WalkError::InvalidMTime(path, _) => path.to_string(),
            WalkError::ChannelDisconnected(_) => "".to_string(),
            WalkError::ChannelRecvError(_) => "".to_string(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            WalkError::FsUtf8Error(_) => "invalid file name encoding".to_string(),
            WalkError::IOError(_, error) => error.to_string(),
            WalkError::RepoPathError(_, error) => error.to_string(),
            WalkError::InvalidFileType(_) => "invalid file type".to_string(),
            WalkError::InvalidMTime(_, error) => format!("invalid mtime - {}", error.to_string()),
            WalkError::ChannelDisconnected(error) => error.to_string(),
            WalkError::ChannelRecvError(error) => error.to_string(),
        }
    }
}

pub enum WalkEntry {
    File(RepoPathBuf, Metadata),
    Directory(RepoPathBuf),
}

impl AsRef<RepoPath> for WalkEntry {
    fn as_ref(&self) -> &RepoPath {
        match self {
            WalkEntry::File(f, _) => f,
            WalkEntry::Directory(d) => d,
        }
    }
}

/// [`Walker`] traverses the working copy, starting at the root of the repo, finding
/// files matched by the matcher.
pub struct Walker<M>(WalkerType<M>);

impl<M> Walker<M>
where
    M: Matcher + Clone + Send + Sync + 'static,
{
    /// Create a new Walker.
    ///
    /// If `num_threads` is 0, a single-threaded Walker will be created. Otherwise, a
    /// multi-threaded walker will be created.
    pub fn new(
        root: PathBuf,
        dot_dir: String,
        matcher: M,
        include_directories: bool,
        num_threads: u8,
    ) -> Result<Self> {
        let inner = match NonZeroU8::new(num_threads) {
            Some(num_threads) => WalkerType::Multi(MultiWalker::new(
                root,
                dot_dir,
                matcher,
                include_directories,
                num_threads,
            )?),
            None => WalkerType::Single(SingleWalker::new(
                root,
                dot_dir,
                matcher,
                include_directories,
            )?),
        };
        Ok(Walker(inner))
    }
}

impl<M> Iterator for Walker<M>
where
    M: Matcher + Clone + Send + Sync + 'static,
{
    type Item = Result<WalkEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        match &mut self.0 {
            WalkerType::Single(w) => w.next(),
            WalkerType::Multi(w) => w.next(),
        }
    }
}

enum WalkerType<M> {
    Single(SingleWalker<M>),
    Multi(MultiWalker<M>),
}

/// SingleWalker traverses the working copy, starting at the root of the repo,
/// finding files matched by matcher
struct SingleWalker<M> {
    root: PathBuf,
    dir_matches: Vec<RepoPathBuf>,
    results: Vec<Result<WalkEntry>>,
    matcher: M,
    include_directories: bool,
    dot_dir: String,
}

impl<M> SingleWalker<M>
where
    M: Matcher,
{
    pub fn new(
        root: PathBuf,
        dot_dir: String,
        matcher: M,
        include_directories: bool,
    ) -> Result<Self> {
        let mut dir_matches = vec![];
        if matcher.matches_directory(&RepoPathBuf::new())? != DirectoryMatch::Nothing {
            dir_matches.push(RepoPathBuf::new());
        }
        let walker = SingleWalker {
            root,
            dir_matches,
            results: Vec::new(),
            matcher,
            include_directories,
            dot_dir,
        };
        Ok(walker)
    }

    fn match_entry(&mut self, next_dir: &RepoPathBuf, entry: DirEntry) -> Result<()> {
        // It'd be nice to move all this conversion noise to a function, but having it here saves
        // us from allocating filename repeatedly.
        let filename = entry.file_name();
        let filename = filename
            .to_str()
            .ok_or_else(|| WalkError::FsUtf8Error(filename.to_string_lossy().into_owned()))?;
        let filename = RepoPath::from_str(filename)
            .map_err(|e| WalkError::RepoPathError(filename.to_owned(), e))?;
        let filetype = entry
            .file_type()
            .map_err(|e| WalkError::IOError(filename.to_owned(), e))?;

        let mut candidate_path = next_dir.clone();
        candidate_path.push(filename);
        if filetype.is_file() || filetype.is_symlink() {
            if self.matcher.matches_file(candidate_path.as_repo_path())? {
                self.results
                    .push(Ok(WalkEntry::File(candidate_path, entry.metadata()?)));
            }
        } else if filetype.is_dir() {
            if filename.as_str() != self.dot_dir
                && self
                    .matcher
                    .matches_directory(candidate_path.as_repo_path())?
                    != DirectoryMatch::Nothing
            {
                self.dir_matches.push(candidate_path);
            }
        } else if self.matcher.matches_file(candidate_path.as_repo_path())? {
            return Err(WalkError::InvalidFileType(filename.to_owned()).into());
        }
        Ok(())
    }

    /// Lazy traversal to find matching files
    fn walk(&mut self) -> Result<()> {
        while self.results.is_empty() && !self.dir_matches.is_empty() {
            let next_dir = self.dir_matches.pop().unwrap();
            if self.include_directories {
                self.results
                    .push(Ok(WalkEntry::Directory(next_dir.clone())));
            }
            let abs_next_dir = self.root.join(next_dir.as_str());
            // Don't process the directory if it contains a .hg directory, unless it's the root.
            if next_dir.is_empty() || !Path::exists(&abs_next_dir.join(&self.dot_dir)) {
                for entry in fs::read_dir(abs_next_dir)
                    .map_err(|e| WalkError::IOError(next_dir.clone(), e))?
                {
                    let entry = entry.map_err(|e| WalkError::IOError(next_dir.clone(), e))?;
                    if let Err(e) = self.match_entry(&next_dir, entry) {
                        self.results.push(Err(e));
                    }
                }
            }
        }
        Ok(())
    }
}

impl<M> Iterator for SingleWalker<M>
where
    M: Matcher,
{
    type Item = Result<WalkEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.walk() {
            Err(e) => Some(Err(e)),
            Ok(()) => self.results.pop(),
        }
    }
}

pub struct WalkerData<M> {
    result_sender: Sender<Result<WalkEntry>>,
    queue_sender: Sender<RepoPathBuf>,
    queue_receiver: Receiver<RepoPathBuf>,
    matcher: M,
    busy_nodes: AtomicU64,
    result_cnt: AtomicU64,
    root: PathBuf,
    include_directories: bool,
}

impl<M> WalkerData<M> {
    fn enqueue_result(&self, msg: Result<WalkEntry, Error>) -> Result<()> {
        self.result_cnt.fetch_add(1, Ordering::AcqRel);
        Ok(self.result_sender.send(msg)?)
    }

    fn enqueue_work(&self, msg: RepoPathBuf) -> Result<()> {
        self.busy_nodes.fetch_add(1, Ordering::AcqRel);
        Ok(self.queue_sender.send(msg)?)
    }
}

struct MultiWalker<M> {
    threads: Vec<JoinHandle<Result<()>>>,
    results: Vec<Result<WalkEntry>>,
    result_receiver: Receiver<Result<WalkEntry>>,
    has_walked: bool,
    payload: Arc<WalkerData<M>>,
    dot_dir: String,
}

impl<M> MultiWalker<M>
where
    M: Matcher,
    M: Clone,
    M: Send,
    M: Sync,
    M: 'static,
{
    const RECV_TIMEOUT: Duration = Duration::from_millis(5);

    pub fn new(
        root: PathBuf,
        dot_dir: String,
        matcher: M,
        include_directories: bool,
        num_threads: NonZeroU8,
    ) -> Result<Self> {
        let (s_results, r_results) = unbounded();
        let (s_queue, r_queue) = unbounded();
        let num_threads = num_threads.get();

        Ok(MultiWalker {
            threads: Vec::with_capacity(num_threads.into()),
            results: Vec::new(),
            result_receiver: r_results,
            has_walked: false,
            payload: Arc::new(WalkerData {
                busy_nodes: AtomicU64::new(0),
                result_cnt: AtomicU64::new(0),
                result_sender: s_results,
                queue_sender: s_queue,
                queue_receiver: r_queue,
                root,
                matcher,
                include_directories,
            }),
            dot_dir,
        })
    }

    // WARNING: SIDE EFFECTS - if entry matches and is child directory, will push
    // child and increment busy_nodes atomic.
    fn match_entry_and_enqueue(
        dir: &RepoPathBuf,
        dot_dir: &str,
        entry: DirEntry,
        shared_data: Arc<WalkerData<M>>,
    ) -> Result<()> {
        let filename = entry.file_name();
        let filename = filename
            .to_str()
            .ok_or_else(|| WalkError::FsUtf8Error(filename.to_string_lossy().into_owned()))?;
        let filename = RepoPath::from_str(filename)
            .map_err(|e| WalkError::RepoPathError(filename.to_owned(), e))?;
        let filetype = entry
            .file_type()
            .map_err(|e| WalkError::IOError(filename.to_owned(), e))?;

        let mut candidate_path = dir.clone();
        candidate_path.push(filename);
        if filetype.is_file() || filetype.is_symlink() {
            if shared_data
                .matcher
                .matches_file(candidate_path.as_repo_path())?
            {
                shared_data
                    .enqueue_result(Ok(WalkEntry::File(candidate_path, entry.metadata()?)))?;
            }
        } else if filetype.is_dir() {
            if filename.as_str() != dot_dir
                && shared_data
                    .matcher
                    .matches_directory(candidate_path.as_repo_path())?
                    != DirectoryMatch::Nothing
            {
                shared_data.enqueue_work(candidate_path)?;
            }
        } else if shared_data
            .matcher
            .matches_file(candidate_path.as_repo_path())?
        {
            return Err(WalkError::InvalidFileType(filename.to_owned()).into());
        }
        Ok(())
    }

    fn walk(&mut self) -> Result<()> {
        if self
            .payload
            .matcher
            .matches_directory(&RepoPathBuf::new())?
            != DirectoryMatch::Nothing
        {
            self.payload.enqueue_work(RepoPathBuf::new())?;
        }

        for _t in 0..self.threads.capacity() {
            let shared_data = self.payload.clone();
            let dot_dir = self.dot_dir.clone();

            // TODO make sure that _t is different for each thread
            self.threads.push(thread::spawn(move || {
                loop {
                    let result = shared_data
                        .queue_receiver
                        .recv_timeout(MultiWalker::<M>::RECV_TIMEOUT);
                    match result {
                        Ok(dir) => {
                            // Anonymous function so we can capture all errors returned, and decrement
                            // busy_nodes even in the event of an error.
                            let result = (|| -> Result<()> {
                                if shared_data.include_directories {
                                    shared_data
                                        .enqueue_result(Ok(WalkEntry::Directory(dir.clone())))?;
                                }
                                let abs_dir_path = shared_data.root.join(dir.as_str());
                                for entry in fs::read_dir(abs_dir_path)
                                    .map_err(|e| WalkError::IOError(dir.clone(), e))?
                                {
                                    let entry =
                                        entry.map_err(|e| WalkError::IOError(dir.clone(), e))?;
                                    if let Err(e) = MultiWalker::match_entry_and_enqueue(
                                        &dir,
                                        &dot_dir,
                                        entry,
                                        shared_data.clone(),
                                    ) {
                                        shared_data.enqueue_result(Err(e))?;
                                    }
                                }
                                Ok(())
                            })();
                            shared_data.busy_nodes.fetch_sub(1, Ordering::AcqRel);
                            result?;
                        }
                        Err(RecvTimeoutError::Timeout) => {
                            if shared_data.busy_nodes.load(Ordering::Relaxed) == 0 {
                                return Ok(());
                            }
                        }
                        Err(e) => {
                            return Err(WalkError::ChannelDisconnected(e).into());
                        }
                    };
                }
            }));
        }

        // TODO in future, let caller handle receiving on the channel.
        for handle in self.threads.drain(0..) {
            let thread_result = handle.join().expect("Failed to join thread.");
            if let Err(e) = thread_result {
                self.results.push(Err(e));
            }
        }
        let cnt = self.payload.result_cnt.load(Ordering::Relaxed);
        for _ in 0..cnt {
            let result = self.result_receiver.recv();

            match result {
                Ok(val) => self.results.push(val),
                // Should not get disconnected because Walker owns a Sender.
                Err(e) => return Err(WalkError::ChannelRecvError(e).into()),
            }
        }
        Ok(())
    }
}

impl<M> Iterator for MultiWalker<M>
where
    M: Matcher,
    M: Clone,
    M: Send,
    M: Sync,
    M: 'static,
{
    type Item = Result<WalkEntry>;
    fn next(&mut self) -> Option<Self::Item> {
        if self.has_walked {
            self.results.pop()
        } else {
            self.has_walked = true;
            match self.walk() {
                Err(e) => Some(Err(e)),
                Ok(()) => self.results.pop(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs::create_dir_all;
    use std::fs::OpenOptions;
    use std::path::PathBuf;

    use pathmatcher::AlwaysMatcher;
    use pathmatcher::NeverMatcher;
    use pathmatcher::TreeMatcher;
    use tempfile::tempdir;

    use super::*;

    fn create_directory(
        directories: &std::vec::Vec<&str>,
        files: &std::vec::Vec<&str>,
    ) -> Result<tempfile::TempDir> {
        let root = tempdir()?;
        for dir in directories {
            create_dir_all(root.path().join(dir))?;
        }
        for file in files {
            let path = root.path().join(file);
            OpenOptions::new()
                .create(true)
                .write(true)
                .open(path.as_path())?;
        }
        Ok(root)
    }

    #[test]
    fn test_singlewalker_nevermatcher() -> Result<()> {
        let directories = vec!["dirA"];
        let files = vec!["dirA/a.txt", "b.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = SingleWalker::new(root_path, ".hg".to_string(), NeverMatcher::new(), false)?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert!(walked_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_singlewalker_alwaysmatcher() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = SingleWalker::new(root_path, ".hg".to_string(), AlwaysMatcher::new(), false)?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_singlewalker_treematcher() -> Result<()> {
        let directories = vec!["foo", "foo/bar"];
        let files = vec!["foo/cat.txt", "foo/bar/baz.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = SingleWalker::new(
            root_path,
            ".hg".to_string(),
            TreeMatcher::from_rules(["foo/bar/**"].iter(), true).unwrap(),
            false,
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let res = vec!["foo/bar/baz.txt"];
        assert_eq!(walked_files.len(), res.len());
        for file in walked_files {
            assert!(res.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_singlewalker_dirs() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = SingleWalker::new(root_path, ".hg".to_string(), AlwaysMatcher::new(), true)?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        // Includes root dir ""
        let res = vec![
            "",
            "dirA",
            "dirA/a.txt",
            "dirA/b.txt",
            "dirB",
            "dirB/dirC",
            "dirB/dirC/dirD",
            "dirB/dirC/dirD/c.txt",
        ];
        assert_eq!(walked_files.len(), res.len());
        for file in walked_files {
            assert!(res.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_multiwalker_nevermatcher() -> Result<()> {
        let directories = vec!["dirA"];
        let files = vec!["dirA/a.txt", "b.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            NeverMatcher::new(),
            false,
            NonZeroU8::new(5).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert!(walked_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_multiwalker_1() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            AlwaysMatcher::new(),
            false,
            NonZeroU8::new(1).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_multiwalker_2() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            AlwaysMatcher::new(),
            false,
            NonZeroU8::new(2).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_multiwalker_u8max() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            AlwaysMatcher::new(),
            false,
            NonZeroU8::new(u8::MAX).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_multiwalker_treematcher() -> Result<()> {
        let directories = vec!["foo", "foo/bar"];
        let files = vec!["foo/cat.txt", "foo/bar/baz.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            TreeMatcher::from_rules(["foo/bar/**"].iter(), true).unwrap(),
            false,
            NonZeroU8::new(4).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let res = vec!["foo/bar/baz.txt"];
        assert_eq!(walked_files.len(), res.len());
        for file in walked_files {
            assert!(res.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_multiwalker_dirs() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = MultiWalker::new(
            root_path,
            ".hg".to_string(),
            AlwaysMatcher::new(),
            true,
            NonZeroU8::new(2).unwrap(),
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        // Includes root dir ""
        let res = vec![
            "",
            "dirA",
            "dirA/a.txt",
            "dirA/b.txt",
            "dirB",
            "dirB/dirC",
            "dirB/dirC/dirD",
            "dirB/dirC/dirD/c.txt",
        ];
        assert_eq!(walked_files.len(), res.len());
        for file in walked_files {
            assert!(res.contains(&file.as_ref().to_string().as_str()));
        }
        Ok(())
    }
}
