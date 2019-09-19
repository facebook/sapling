// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

use std::fs::{self, DirEntry};
use std::io;
use std::path::PathBuf;

use failure::Fallible;

use pathmatcher::{DirectoryMatch, Matcher};
use types::{RepoPath, RepoPathBuf};

/// Walker traverses the working copy, starting at the root of the repo,
/// finding files matched by matcher
pub struct Walker<'a, M> {
    root: PathBuf,
    dir_matches: Vec<RepoPathBuf>,
    file_matches: Vec<Fallible<RepoPathBuf>>,
    matcher: &'a M,
}

impl<'a, M> Walker<'a, M>
where
    M: Matcher,
{
    pub fn new(root: PathBuf, matcher: &'a M) -> Self {
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

    fn match_entry(&mut self, next_dir: &RepoPathBuf, entry: io::Result<DirEntry>) -> Fallible<()> {
        let entry = entry?;
        let filename = entry.file_name();
        let filename = RepoPath::from_str(filename.to_str().unwrap())?;
        let filetype = entry.file_type()?;
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
    fn walk(&mut self) -> Fallible<()> {
        while self.file_matches.is_empty() && !self.dir_matches.is_empty() {
            let mut next_dir = self.dir_matches.pop().unwrap();
            for entry in fs::read_dir(self.root.join(next_dir.as_str()))? {
                if let Err(e) = self.match_entry(&mut next_dir, entry) {
                    self.file_matches.push(Err(e));
                }
            }
        }
        Ok(())
    }
}

impl<'a, M> Iterator for Walker<'a, M>
where
    M: Matcher,
{
    type Item = Fallible<RepoPathBuf>;
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
    ) -> Fallible<tempfile::TempDir> {
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
    fn test_walker() -> Fallible<()> {
        let directories = vec!["dirA", "dirB/dirC/dirD"];
        let files = vec!["dirA/a.txt", "dirA/b.txt", "dirB/dirC/dirD/c.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let matcher = AlwaysMatcher::new();
        let walker = Walker::new(root_path, &matcher);
        let walked_files: Result<Vec<_>, _> = walker.collect();
        let walked_files = walked_files?;
        assert_eq!(walked_files.len(), 3);
        for file in walked_files {
            assert!(files.contains(&file.into_string().as_str()));
        }
        Ok(())
    }

    #[test]
    fn test_match_nothing() -> Fallible<()> {
        let directories = vec!["dirA"];
        let files = vec!["dirA/a.txt", "b.txt"];
        let root_dir = create_directory(&directories, &files)?;
        let root_path = PathBuf::from(root_dir.path());
        let matcher = NeverMatcher::new();
        let walker = Walker::new(root_path, &matcher);
        let walked_files: Vec<_> = walker.collect();
        assert!(walked_files.is_empty());
        Ok(())
    }
}
