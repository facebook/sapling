/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::fmt;

use types::RepoPath;
use types::RepoPathBuf;

#[derive(Default)]
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

    pub fn status(&self, file: &RepoPath) -> Option<FileStatus> {
        self.all.get(file).copied()
    }

    fn filter_status(&self, status: FileStatus) -> impl Iterator<Item = &RepoPathBuf> {
        self.all
            .iter()
            .filter_map(move |(f, s)| if *s == status { Some(f) } else { None })
    }
}

#[derive(Clone, Copy, Eq, PartialEq)]
pub enum FileStatus {
    Modified,
    Added,
    Removed,
    Deleted,
    Unknown,
}

impl FileStatus {
    pub fn py_letter(&self) -> &'static str {
        match self {
            FileStatus::Modified => "M",
            FileStatus::Added => "A",
            FileStatus::Removed => "R",
            FileStatus::Unknown => "?",
            FileStatus::Deleted => "!",
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
