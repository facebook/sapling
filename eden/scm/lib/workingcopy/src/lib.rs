/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::fs::{self, DirEntry};
use std::io;
use std::path::{Path, PathBuf};

use anyhow::Result;
use thiserror::Error;

use pathmatcher::{DirectoryMatch, Matcher};
use types::path::ParseError;
use types::{RepoPath, RepoPathBuf};

#[derive(Error, Debug)]
pub enum WalkError {
    #[error("invalid file name encoding '{0}'")]
    FsUtf8Error(String),
    #[error("IO error at '{0}': {1}")]
    IOError(RepoPathBuf, #[source] io::Error),
    #[error("path error at '{0}': {1}")]
    RepoPathError(String, #[source] ParseError),
}

impl WalkError {
    pub fn filename(&self) -> String {
        match self {
            WalkError::FsUtf8Error(path) => path.to_string(),
            WalkError::IOError(path, _) => path.to_string(),
            WalkError::RepoPathError(path, _) => path.to_string(),
        }
    }

    pub fn message(&self) -> String {
        match self {
            WalkError::FsUtf8Error(_) => "invalid file name encoding".to_string(),
            WalkError::IOError(_, error) => error.to_string(),
            WalkError::RepoPathError(_, error) => error.to_string(),
        }
    }
}

/// Walker traverses the working copy, starting at the root of the repo,
/// finding files matched by matcher
pub struct Walker<M> {
    root: PathBuf,
    dir_matches: Vec<RepoPathBuf>,
    file_matches: Vec<Result<RepoPathBuf>>,
    matcher: M,
}

impl<M> Walker<M>
where
    M: Matcher,
{
    pub fn new(root: PathBuf, matcher: M) -> Self {
        let mut dir_matches = vec![];
        if matcher.matches_directory(&RepoPathBuf::new()) != DirectoryMatch::Nothing {
            dir_matches.push(RepoPathBuf::new());
        }
        Walker {
            root,
            dir_matches,
            file_matches: Vec::new(),
            matcher,
        }
    }

    fn match_entry(&mut self, next_dir: &RepoPathBuf, entry: DirEntry) -> Result<()> {
        // It'd be nice to move all this conversion noise to a function, but having it here saves
        // us from allocating filename repeatedly.
        let filename = entry.file_name();
        let filename = filename.to_str().ok_or(WalkError::FsUtf8Error(
            filename.to_string_lossy().into_owned(),
        ))?;
        let filename = RepoPath::from_str(filename)
            .map_err(|e| WalkError::RepoPathError(filename.to_owned(), e))?;
        let filetype = entry
            .file_type()
            .map_err(|e| WalkError::IOError(filename.to_owned(), e))?;

        let mut candidate_path = next_dir.clone();
        candidate_path.push(filename);
        if filetype.is_file() || filetype.is_symlink() {
            if self.matcher.matches_file(candidate_path.as_repo_path()) {
                self.file_matches.push(Ok(candidate_path));
            }
        } else if filetype.is_dir() {
            if filename.as_str() != ".hg"
                && self
                    .matcher
                    .matches_directory(candidate_path.as_repo_path())
                    != DirectoryMatch::Nothing
            {
                self.dir_matches.push(candidate_path);
            }
        }
        Ok(())
    }

    /// Lazy traversal to find matching files
    fn walk(&mut self) -> Result<()> {
        while self.file_matches.is_empty() && !self.dir_matches.is_empty() {
            let next_dir = self.dir_matches.pop().unwrap();
            let abs_next_dir = self.root.join(next_dir.as_str());
            // Don't process the directory if it contains a .hg directory, unless it's the root.
            if next_dir.is_empty() || !Path::exists(&abs_next_dir.join(".hg")) {
                for entry in fs::read_dir(abs_next_dir)
                    .map_err(|e| WalkError::IOError(next_dir.clone(), e))?
                {
                    let entry = entry.map_err(|e| WalkError::IOError(next_dir.clone(), e))?;
                    if let Err(e) = self.match_entry(&next_dir, entry) {
                        self.file_matches.push(Err(e));
                    }
                }
            }
        }
        Ok(())
    }
}

impl<M> Iterator for Walker<M>
where
    M: Matcher,
{
    type Item = Result<RepoPathBuf>;
    fn next(&mut self) -> Option<Self::Item> {
        match self.walk() {
            Err(e) => Some(Err(e)),
            Ok(()) => self.file_matches.pop(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::fs::{create_dir_all, OpenOptions};
    use std::path::PathBuf;

    use tempfile::tempdir;

    use pathmatcher::{AlwaysMatcher, NeverMatcher};

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
    fn test_walker() -> Result<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = Walker::new(root_path, AlwaysMatcher::new());
        let walked_files: Result<Vec<_>> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.into_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_match_nothing() -> Result<()> {
        let directories = vec!["dirA"];
        let files = vec!["dirA/a.txt", "b.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let walker = Walker::new(root_path, NeverMatcher::new());
        let walked_files: Vec<_> = walker.collect();
        assert!(walked_files.is_empty());
        Ok(())
    }
}
