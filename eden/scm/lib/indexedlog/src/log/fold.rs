/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;
use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::Path;

use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::errors::IoResultExt;
use crate::log::Log;
use crate::utils::atomic_write_plain;
use crate::utils::xxhash;
use crate::Error;
use crate::Result;

/// Definition of a "fold" function.
///
/// The "fold" function is similar to the "fold" function in stdlib.
/// It takes an initial state, processes new entries, then output
/// the new state.
///
/// Given an append-only `Log`, the state of a "fold" function can
/// be saved to disk and loaded later. A `Log` maintains the on-disk
/// state so "fold" calculation won't always start from scratch.
#[derive(Clone, Debug)]
pub struct FoldDef {
    /// Function to create an empty fold state.
    pub(crate) create_fold: fn() -> Box<dyn Fold>,

    /// Name of the fold state.
    ///
    /// The name will be used as part of the fold file name. Therefore do not
    /// use user-generated content here. And do not abuse this by using `..` or `/`.
    ///
    /// When adding new or changing fold functions, use a different
    /// `name` to avoid reusing existing data incorrectly.
    pub(crate) name: &'static str,
}

/// The actual logic of a "fold" function, and its associated state.
pub trait Fold: Debug + 'static + Send + Sync {
    /// Load the initial fold state.
    /// This will be called if the state exists.
    fn load(&mut self, state_bytes: &[u8]) -> io::Result<()>;

    /// Dump the fold state as bytes.
    fn dump(&self) -> io::Result<Vec<u8>>;

    /// Process a log entry. Update self state in place.
    fn accumulate(&mut self, entry: &[u8]) -> Result<()>;

    /// Downcast. Useful to access internal state without serialization cost.
    fn as_any(&self) -> &dyn Any;

    /// Clone the state.
    fn clone_boxed(&self) -> Box<dyn Fold>;
}

/// State tracking the progress of a fold function.
#[derive(Debug)]
pub(crate) struct FoldState {
    /// Epoch. Useful to detect non-append-only changes.
    /// See also `LogMetadata`.
    pub(crate) epoch: u64,

    /// Offset of the next entry.
    pub(crate) offset: u64,

    /// The state of the actual fold.
    pub(crate) fold: Box<dyn Fold>,

    /// How to reset the `fold` state.
    def: FoldDef,
}

impl FoldDef {
    /// Create a "fold" definition.
    ///
    /// `create_func` is a function to produce an empty "fold" state.
    pub fn new(name: &'static str, create_fold: fn() -> Box<dyn Fold>) -> Self {
        Self { create_fold, name }
    }

    pub(crate) fn empty_state(&self) -> FoldState {
        FoldState {
            epoch: 0,
            offset: 0,
            fold: (self.create_fold)(),
            def: self.clone(),
        }
    }
}

impl Clone for FoldState {
    fn clone(&self) -> Self {
        Self {
            epoch: self.epoch,
            offset: self.offset,
            fold: self.fold.clone_boxed(),
            def: self.def.clone(),
        }
    }
}

impl FoldState {
    pub(crate) fn load_from_file(&mut self, path: &Path) -> crate::Result<()> {
        (|| -> io::Result<()> {
            let data = fs::read(path)?;
            let checksum = match data.get(0..8) {
                Some(h) => u64::from_be_bytes(<[u8; 8]>::try_from(h).unwrap()),
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        format!("corrupted FoldState (no checksum): {:?}", data),
                    ));
                }
            };
            if xxhash(&data[8..]) != checksum {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    format!("corrupted FoldState (wrong checksum): {:?}", data),
                ));
            }
            let mut reader = &data[8..];
            let epoch = reader.read_vlq()?;
            let offset = reader.read_vlq()?;
            self.fold.load(reader)?;
            self.epoch = epoch;
            self.offset = offset;
            Ok(())
        })()
        .context(path, "cannot read FoldState")
    }

    pub(crate) fn save_to_file(&self, path: &Path) -> crate::Result<()> {
        let data = (|| -> io::Result<Vec<u8>> {
            let mut body = Vec::new();
            body.write_vlq(self.epoch)?;
            body.write_vlq(self.offset)?;
            body.extend_from_slice(&self.fold.dump()?);
            let checksum = xxhash(&body);
            let mut data: Vec<u8> = checksum.to_be_bytes().to_vec();
            data.extend_from_slice(&body);
            Ok(data)
        })()
        .context(path, "cannot prepare FoldState")?;
        atomic_write_plain(path, &data, false)
    }

    /// Ensure the fold state is up-to-date with all on-disk entries.
    ///
    /// Read and write to on-disk caches transparently.
    pub(crate) fn catch_up_with_log_on_disk_entries(&mut self, log: &Log) -> crate::Result<()> {
        // Already up-to-date?
        if self.offset == log.disk_buf.len() as u64 && self.epoch == log.meta.epoch {
            return Ok(());
        }

        // Load from disk.
        let opt_path = log
            .dir
            .as_opt_path()
            .map(|p| p.join(format!("fold-{}", self.def.name)));
        if let Some(path) = &opt_path {
            if let Err(e) = self.load_from_file(path) {
                tracing::warn!("cannot load FoldState: {}", e);
            }
        }

        // Invalidate if mismatch.
        if self.offset > log.disk_buf.len() as u64 || self.epoch != log.meta.epoch {
            self.reset();
        }
        self.epoch = log.meta.epoch;

        // Already up-to-date? (after loading from disk).
        // If so, avoid complexities writing back to disk.
        // Note mismatch epoch would reset offset to 0 above.
        if self.offset == log.disk_buf.len() as u64 {
            return Ok(());
        }

        // Catch up by processing remaining entries one by one.
        let mut iter = log.iter();
        if self.offset > 0 {
            iter.next_offset = self.offset;
        }
        for entry in iter {
            let entry = entry?;
            self.fold.accumulate(entry)?;
        }

        // Set self state as up-to-date, and write to disk.
        self.offset = log.disk_buf.len() as u64;
        if let Some(path) = &opt_path {
            if let Err(e) = self.save_to_file(path) {
                tracing::warn!("cannot save FoldState: {}", e);
            }
        }

        Ok(())
    }

    /// Process the next unprocessed entry.
    ///
    /// `offset` is the offset to the given entry.
    /// `next_offset` is the offset to the next entry.
    ///
    /// The given entry must be the next one to be processed. All previous
    /// entries are already processed and none of the entries after the given
    /// entry are processed.
    pub(crate) fn process_entry(
        &mut self,
        entry: &[u8],
        offset: u64,
        next_offset: u64,
    ) -> crate::Result<()> {
        if self.offset != offset {
            return Err(Error::programming(format!(
                "FoldState got mismatched offset: {:?} != {:?}",
                self.offset, offset
            )));
        }
        self.fold.accumulate(entry)?;
        self.offset = next_offset;
        Ok(())
    }

    fn reset(&mut self) {
        self.offset = 0;
        self.fold = (self.def.create_fold)();
    }
}

#[cfg(test)]
mod test {
    use tempfile::tempdir;

    use super::*;

    #[derive(Debug, Default)]
    struct ConcatFold(Vec<u8>);

    impl Fold for ConcatFold {
        fn load(&mut self, state_bytes: &[u8]) -> io::Result<()> {
            self.0 = state_bytes.to_vec();
            Ok(())
        }

        fn dump(&self) -> io::Result<Vec<u8>> {
            Ok(self.0.clone())
        }

        fn accumulate(&mut self, entry: &[u8]) -> Result<()> {
            self.0.extend_from_slice(entry);
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn clone_boxed(&self) -> Box<dyn Fold> {
            Box::new(Self(self.0.clone()))
        }
    }

    #[derive(Debug, Default)]
    struct CountFold(u64);

    impl Fold for CountFold {
        fn load(&mut self, state_bytes: &[u8]) -> io::Result<()> {
            let bytes = <[u8; 8]>::try_from(state_bytes).unwrap();
            let count = u64::from_be_bytes(bytes);
            self.0 = count;
            Ok(())
        }

        fn dump(&self) -> io::Result<Vec<u8>> {
            Ok(self.0.to_be_bytes().to_vec())
        }

        fn accumulate(&mut self, _entry: &[u8]) -> Result<()> {
            self.0 += 1;
            Ok(())
        }

        fn as_any(&self) -> &dyn Any {
            self
        }

        fn clone_boxed(&self) -> Box<dyn Fold> {
            Box::new(Self(self.0))
        }
    }

    #[test]
    fn test_fold_state_load_save() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("foo");
        let def = FoldDef::new("foo", || Box::new(ConcatFold::default()));
        let d = |v: &FoldState| format!("{:?}", v);

        let mut state1 = def.empty_state();
        let mut state2 = def.empty_state();

        // Check empty state round-trip.
        state1.save_to_file(&path).unwrap();
        state2.load_from_file(&path).unwrap();
        assert_eq!(d(&state1), d(&state2));

        // Check some state round-trip.
        state1.epoch = 10;
        state1.offset = 20;
        state1.fold.accumulate(b"abc").unwrap();
        state1.fold.accumulate(b"def").unwrap();
        state2.fold.accumulate(b"ghi").unwrap();
        state1.save_to_file(&path).unwrap();
        state2.load_from_file(&path).unwrap();
        assert_eq!(d(&state1), d(&state2));
    }

    #[test]
    fn test_fold_on_log() {
        let dir = tempdir().unwrap();
        let path = dir.path();

        // Prepare 2 logs.
        let opts = crate::log::OpenOptions::new()
            .fold_def("m", || Box::new(ConcatFold::default()))
            .fold_def("c", || Box::new(CountFold::default()))
            .create(true);
        let mut log1 = opts.open(path).unwrap();
        let mut log2 = log1.try_clone().unwrap();

        // Helper to read fold results. f1: ConcatFold; f2: CountFold.
        let f1 = |log: &Log| {
            log.fold(0)
                .unwrap()
                .as_any()
                .downcast_ref::<ConcatFold>()
                .unwrap()
                .0
                .clone()
        };
        let f2 = |log: &Log| {
            log.fold(1)
                .unwrap()
                .as_any()
                .downcast_ref::<CountFold>()
                .unwrap()
                .0
        };

        // Empty logs.
        assert_eq!(f1(&log1), b"");
        assert_eq!(f2(&log2), 0);

        // Different in-memory entries.
        log1.append(b"ab").unwrap();
        log1.append(b"cd").unwrap();
        log2.append(b"e").unwrap();
        log2.append(b"f").unwrap();
        assert_eq!(f1(&log1), b"abcd");
        assert_eq!(f2(&log1), 2);
        assert_eq!(f1(&log2), b"ef");
        assert_eq!(f2(&log2), 2);

        // Write to disk. log2 will pick up log1 entries.
        log1.sync().unwrap();
        log2.sync().unwrap();
        assert_eq!(f1(&log1), b"abcd");
        assert_eq!(f2(&log1), 2);
        assert_eq!(f1(&log2), b"abcdef");
        assert_eq!(f2(&log2), 4);

        // With new in-memory entries.
        log1.append(b"x").unwrap();
        log2.append(b"y").unwrap();
        assert_eq!(f1(&log1), b"abcdx");
        assert_eq!(f2(&log1), 3);
        assert_eq!(f1(&log2), b"abcdefy");
        assert_eq!(f2(&log2), 5);

        // Clone with and without pending entries.
        let log3 = log1.try_clone_without_dirty().unwrap();
        assert_eq!(f1(&log3), b"abcd");
        assert_eq!(f2(&log3), 2);
        let log3 = log1.try_clone().unwrap();
        assert_eq!(f1(&log3), b"abcdx");
        assert_eq!(f2(&log3), 3);

        // Write to disk again.
        log2.sync().unwrap();
        log1.sync().unwrap();
        assert_eq!(f1(&log1), b"abcdefyx");
        assert_eq!(f2(&log1), 6);
        assert_eq!(f1(&log2), b"abcdefy");
        assert_eq!(f2(&log2), 5);

        // Sync with read fast path.
        log2.sync().unwrap();
        assert_eq!(f1(&log2), b"abcdefyx");
        assert_eq!(f2(&log2), 6);

        // Corrupted folds are simply ignored instead of causing errors.
        fs::write(path.join("fold-m"), b"corruptedcontent").unwrap();
        fs::write(path.join("fold-c"), b"\0\0\0\0\0\0\0\0\0").unwrap();
        let mut log3 = opts.open(path).unwrap();
        assert_eq!(f1(&log3), b"abcdefyx");
        assert_eq!(f2(&log3), 6);
        log3.sync().unwrap();
        assert_eq!(f1(&log3), b"abcdefyx");
        assert_eq!(f2(&log3), 6);
    }
}
