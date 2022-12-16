/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;
use std::path::Path;
use std::path::PathBuf;

use types::RepoPath;
use types::RepoPathBuf;

#[derive(Default, Clone, PartialEq, Eq)]
pub struct Status {
    all: HashMap<RepoPathBuf, FileStatus>,
}

pub struct StatusBuilder(Status);

impl StatusBuilder {
    pub fn new() -> Self {
        Self(Status::default())
    }

    pub fn build(self) -> Status {
        self.0
    }

    pub fn modified(mut self, modified: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, modified, FileStatus::Modified);
        self
    }

    pub fn added(mut self, added: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, added, FileStatus::Added);
        self
    }

    pub fn removed(mut self, removed: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, removed, FileStatus::Removed);
        self
    }

    pub fn deleted(mut self, deleted: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, deleted, FileStatus::Deleted);
        self
    }

    pub fn unknown(mut self, unknown: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, unknown, FileStatus::Unknown);
        self
    }

    pub fn ignored(mut self, ignored: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, ignored, FileStatus::Ignored);
        self
    }

    pub fn clean(mut self, clean: Vec<RepoPathBuf>) -> Self {
        Self::index(&mut self.0.all, clean, FileStatus::Clean);
        self
    }

    pub fn iter(&self) -> impl Iterator<Item = (&RepoPath, FileStatus)> {
        self.0.iter()
    }

    // This fn has to take 'deconstructed' self, because you can't borrow &mut self and &self.xxx at the same time
    fn index(
        all: &mut HashMap<RepoPathBuf, FileStatus>,
        files: Vec<RepoPathBuf>,
        status: FileStatus,
    ) {
        for file in files {
            all.insert(file, status);
        }
    }
}

impl Status {
    // modified() and other functions intentionally return Iterator<> and not &Vec
    // Those functions can be used if someone needs a list of files in certain category
    // If someone need to check what is the status of a file, they should use status(file) because it handles case sensitivity properly
    pub fn modified(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Modified)
    }

    pub fn added(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Added)
    }

    pub fn removed(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Removed)
    }

    pub fn deleted(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Deleted)
    }

    pub fn unknown(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Unknown)
    }

    pub fn ignored(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Ignored)
    }

    pub fn clean(&self) -> impl Iterator<Item = &RepoPathBuf> {
        self.filter_status(FileStatus::Clean)
    }

    pub fn status(&self, file: &RepoPath) -> Option<FileStatus> {
        self.all.get(file).copied()
    }

    fn filter_status(&self, status: FileStatus) -> impl Iterator<Item = &RepoPathBuf> {
        self.all
            .iter()
            .filter_map(move |(f, s)| if *s == status { Some(f) } else { None })
    }

    pub fn iter(&self) -> impl Iterator<Item = (&RepoPath, FileStatus)> {
        self.all.iter().map(|(f, s)| (f.as_repo_path(), *s))
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileStatus {
    /// The file has been modified.
    Modified,
    /// The file has been added to the tree.
    Added,
    /// The file has been removed to the tree.
    Removed,
    /// The file is in the tree, but it isn't in the working copy (it's "missing").
    Deleted,
    /// The file isn't in the tree, but it exists in the working copy and it isn't ignored.
    Unknown,
    /// The file isn't in the tree and it exists in the working copy, but it is ignored.
    Ignored,
    /// The file has not been modified.
    Clean,
}

impl FileStatus {
    pub fn py_letter(&self) -> &'static str {
        match self {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Removed => "R",
            FileStatus::Deleted => "!",
            FileStatus::Unknown => "?",
            FileStatus::Ignored => "I",
            FileStatus::Clean => "C",
        }
    }
}

impl fmt::Display for Status {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        for (file, status) in self.all.iter() {
            write!(f, "{} {}", status.py_letter(), file)?;
        }
        Ok(())
    }
}

pub fn needs_morestatus_extension(hg_dir: &Path, parent_count: usize) -> bool {
    if parent_count > 1 {
        return true;
    }

    for path in [
        PathBuf::from("bisect.state"),
        PathBuf::from("graftstate"),
        PathBuf::from("histedit-state"),
        PathBuf::from("merge/state"),
        PathBuf::from("rebasestate"),
        PathBuf::from("unshelverebasestate"),
        PathBuf::from("updatestate"),
    ] {
        if hg_dir.join(path).is_file() {
            return true;
        }
    }

    false
}
