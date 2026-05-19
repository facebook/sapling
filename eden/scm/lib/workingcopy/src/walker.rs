/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::thread;
use std::thread::JoinHandle;
use std::time::Duration;

use anyhow::Error;
use anyhow::Result;
use crossbeam::channel::Receiver;
use crossbeam::channel::RecvError;
use crossbeam::channel::RecvTimeoutError;
use crossbeam::channel::Sender;
use crossbeam::channel::unbounded;
use pathmatcher::DirectoryMatch;
use pathmatcher::Matcher;
use thiserror::Error;
use types::RepoPath;
use types::RepoPathBuf;
use types::path::ParseError;
use vfs::AuditError;
use vfs::LiteMetadata;
use vfs::VFS;

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
            WalkError::InvalidMTime(_, error) => format!("invalid mtime - {}", error),
            WalkError::ChannelDisconnected(error) => error.to_string(),
            WalkError::ChannelRecvError(error) => error.to_string(),
        }
    }
}

pub enum WalkEntry {
    File(RepoPathBuf, LiteMetadata),
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

pub struct WalkerData<M> {
    result_sender: Sender<Result<WalkEntry>>,
    queue_sender: Sender<RepoPathBuf>,
    queue_receiver: Receiver<RepoPathBuf>,
    matcher: M,
    busy_nodes: AtomicU64,
    result_cnt: AtomicU64,
    vfs: VFS,
    include_directories: bool,
    dot_dir: String,
    skip_dirs: HashSet<RepoPathBuf>,
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

pub struct Walker<M> {
    threads: Vec<JoinHandle<Result<()>>>,
    results: Vec<Result<WalkEntry>>,
    result_receiver: Receiver<Result<WalkEntry>>,
    has_walked: bool,
    payload: Arc<WalkerData<M>>,
}

impl<M> Walker<M>
where
    M: Matcher,
    M: Clone,
    M: Send,
    M: Sync,
    M: 'static,
{
    const RECV_TIMEOUT: Duration = Duration::from_millis(5);

    pub fn new(
        vfs: VFS,
        dot_dir: String,
        skip_dirs: Vec<PathBuf>,
        matcher: M,
        include_directories: bool,
    ) -> Result<Self> {
        let (s_results, r_results) = unbounded();
        let (s_queue, r_queue) = unbounded();

        Ok(Walker {
            threads: Vec::with_capacity(8),
            results: Vec::new(),
            result_receiver: r_results,
            has_walked: false,
            payload: Arc::new(WalkerData {
                busy_nodes: AtomicU64::new(0),
                result_cnt: AtomicU64::new(0),
                result_sender: s_results,
                queue_sender: s_queue,
                queue_receiver: r_queue,
                vfs,
                matcher,
                include_directories,
                // dot_dir is only used to avoid walking into nested repos.
                // If dot_dir is ".git/sl". Then turn it into ".git".
                dot_dir: if dot_dir.contains('/') {
                    dot_dir.split('/').next().unwrap_or("").to_string()
                } else {
                    dot_dir
                },
                skip_dirs: skip_dirs
                    .into_iter()
                    .map(|p| Ok(p.try_into()?))
                    .collect::<Result<_>>()?,
            }),
        })
    }

    // WARNING: SIDE EFFECTS - if entry matches and is child directory, will push
    // child and increment busy_nodes atomic.
    fn match_entry_and_enqueue(
        dir: &RepoPathBuf,
        filename: &RepoPath,
        shared_data: Arc<WalkerData<M>>,
    ) -> Result<()> {
        let mut candidate_path = dir.clone();
        candidate_path.push(filename);
        let metadata = match shared_data.vfs.metadata(candidate_path.as_repo_path()) {
            Ok(metadata) => metadata,
            Err(err) if is_invalid_component_error(&err) => return Ok(()),
            Err(err) => return Err(err),
        };
        if metadata.is_file() || metadata.is_symlink() {
            if shared_data
                .matcher
                .matches_file(candidate_path.as_repo_path())?
            {
                shared_data.enqueue_result(Ok(WalkEntry::File(candidate_path, metadata)))?;
            }
        } else if metadata.is_dir() {
            if !shared_data.skip_dirs.contains(filename)
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

            // TODO make sure that _t is different for each thread
            self.threads.push(thread::spawn(move || {
                loop {
                    let result = shared_data
                        .queue_receiver
                        .recv_timeout(Walker::<M>::RECV_TIMEOUT);
                    match result {
                        Ok(dir) => {
                            // Anonymous function so we can capture all errors returned, and decrement
                            // busy_nodes even in the event of an error.
                            let result = (|| -> Result<()> {
                                if shared_data.include_directories {
                                    shared_data
                                        .enqueue_result(Ok(WalkEntry::Directory(dir.clone())))?;
                                }

                                if !dir.is_empty() {
                                    let dot_dir = RepoPath::from_str(&shared_data.dot_dir)
                                        .map_err(|err| {
                                            WalkError::RepoPathError(
                                                shared_data.dot_dir.clone(),
                                                err,
                                            )
                                        })?;
                                    let mut dot_dir_path = dir.clone();
                                    dot_dir_path.push(dot_dir);
                                    if shared_data
                                        .vfs
                                        .raw_no_follow_root()?
                                        .symlink_metadata(Some(dot_dir_path.as_repo_path()))
                                        .is_ok()
                                    {
                                        return Ok(());
                                    }
                                }

                                for filename in shared_data.vfs.list_dir(dir.as_repo_path())? {
                                    let filename = match filename {
                                        Ok(filename) => filename.into_string(),
                                        Err(err) => {
                                            shared_data.enqueue_result(Err(err))?;
                                            continue;
                                        }
                                    };
                                    let repo_filename = match RepoPath::from_str(&filename) {
                                        Ok(filename) => filename,
                                        Err(err) => {
                                            shared_data.enqueue_result(Err(
                                                WalkError::RepoPathError(filename, err).into(),
                                            ))?;
                                            continue;
                                        }
                                    };
                                    if let Err(e) = Walker::match_entry_and_enqueue(
                                        &dir,
                                        repo_filename,
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

fn is_invalid_component_error(err: &Error) -> bool {
    matches!(
        err.downcast_ref::<AuditError>(),
        Some(AuditError::InvalidComponent(_, _))
    )
}

impl<M> Iterator for Walker<M>
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
    use fs_err::OpenOptions;
    use fs_err::create_dir_all;
    use pathmatcher::AlwaysMatcher;
    use pathmatcher::NeverMatcher;
    use pathmatcher::TreeMatcher;
    use tempfile::tempdir;
    use vfs::VFS;

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
    fn test_multiwalker_nevermatcher() -> Result<()> {
        let directories = vec!["dirA"];
        let files = vec!["dirA/a.txt", "b.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let vfs = VFS::new(root_dir.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            Vec::new(),
            NeverMatcher::new(),
            false,
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert!(walked_files.is_empty());
        Ok(())
    }

    #[test]
    fn test_multiwalker_treematcher() -> Result<()> {
        let directories = vec!["foo", "foo/bar"];
        let files = vec!["foo/cat.txt", "foo/bar/baz.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let vfs = VFS::new(root_dir.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            Vec::new(),
            TreeMatcher::from_rules(["foo/bar/**"].iter(), true).unwrap(),
            false,
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let res = ["foo/bar/baz.txt"];
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
        let vfs = VFS::new(root_dir.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            Vec::new(),
            AlwaysMatcher::new(),
            true,
        )?;
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        // Includes root dir ""
        let res = [
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
    fn test_multiwalker_skips_nested_repo() -> Result<()> {
        let directories = vec!["nested/.sl"];
        let files = vec!["nested/file.txt", "root.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let vfs = VFS::new(root_dir.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            Vec::new(),
            AlwaysMatcher::new(),
            false,
        )?;

        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let walked_files: HashSet<_> = walked_files
            .into_iter()
            .map(|file| file.as_ref().to_string())
            .collect();

        assert!(walked_files.contains("root.txt"));
        assert!(!walked_files.contains("nested/file.txt"));
        Ok(())
    }

    #[test]
    fn test_multiwalker_ignores_invalid_vfs_components() -> Result<()> {
        let directories = vec![".sl"];
        let files = vec![".sl/requires", "root.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let vfs = VFS::new(root_dir.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            vec![PathBuf::from(".sl")],
            AlwaysMatcher::new(),
            false,
        )?;

        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let walked_files: HashSet<_> = walked_files
            .into_iter()
            .map(|file| file.as_ref().to_string())
            .collect();

        assert!(walked_files.contains("root.txt"));
        assert!(!walked_files.contains(".sl/requires"));
        Ok(())
    }

    #[cfg(unix)]
    #[test]
    fn test_multiwalker_does_not_follow_directory_symlink() -> Result<()> {
        let root = tempdir()?;
        create_dir_all(root.path().join("target"))?;
        OpenOptions::new()
            .create(true)
            .write(true)
            .open(root.path().join("target/file.txt"))?;
        std::os::unix::fs::symlink("target", root.path().join("link"))?;
        let vfs = VFS::new(root.path().to_path_buf())?;
        let walker = Walker::new(
            vfs,
            ".sl".to_string(),
            Vec::new(),
            AlwaysMatcher::new(),
            false,
        )?;

        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        let walked_files: HashSet<_> = walked_files
            .into_iter()
            .map(|file| file.as_ref().to_string())
            .collect();

        assert!(walked_files.contains("link"));
        assert!(walked_files.contains("target/file.txt"));
        assert!(!walked_files.contains("link/file.txt"));
        Ok(())
    }
}
