// Copyright 2019 Facebook, Inc.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

//! Rotation support for a set of [Log]s.

use atomicwrites::{AllowOverwrite, AtomicFile};
use lock::ScopedFileLock;
use log::{self, IndexDef, Log};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// A collection of [Log]s that get rotated or deleted automatically when they
/// exceed size or count limits.
///
/// Writes go to the active [Log]. Reads scan through all [Log]s.
pub struct LogRotate {
    dir: PathBuf,
    open_options: OpenOptions,
    logs: Vec<Log>,
    latest: u64,
}

// On disk, a LogRotate is a directory containing:
// - 0/, 1/, 2/, 3/, ...: one Log per directory.
// - latest: a file, the name of the directory that is considered "active".

const LATEST_FILE: &str = "latest";

/// Options used to configure how a [LogRotate] is opened.
pub struct OpenOptions {
    max_bytes_per_log: u64,
    max_log_count: u64,
    create: bool,
    index_defs: Vec<IndexDef>,
}

impl OpenOptions {
    /// Creates a default new set of options ready for configuration.
    ///
    /// The default values are:
    /// - Keep 2 logs.
    /// - A log gets rotated when it exceeds 2GB.
    /// - No indexes.
    /// - Do not create on demand.
    pub fn new() -> Self {
        // Some "seemingly reasonable" default values. Not scientifically chosen.
        let max_log_count = 2;
        let max_bytes_per_log = 2_000_000_000; // 2 GB
        Self {
            max_bytes_per_log,
            max_log_count,
            index_defs: Vec::new(),
            create: false,
        }
    }

    /// Set the maximum [Log] count.
    pub fn max_log_count(mut self, count: u64) -> Self {
        assert!(count >= 1);
        self.max_log_count = count;
        self
    }

    /// Set the maximum bytes per [Log].
    pub fn max_bytes_per_log(mut self, bytes: u64) -> Self {
        assert!(bytes > 0);
        self.max_bytes_per_log = bytes;
        self
    }

    /// Set whether create the [LogRotate] structure if it does not exist.
    pub fn create(mut self, create: bool) -> Self {
        self.create = create;
        self
    }

    /// Set the index definitions.
    ///
    /// See [IndexDef] for details.
    pub fn index_defs(mut self, index_defs: Vec<IndexDef>) -> Self {
        self.index_defs = index_defs;
        self
    }

    /// Open [LogRotate] at given location.
    pub fn open(self, dir: impl AsRef<Path>) -> io::Result<LogRotate> {
        unimplemented!()
    }
}
