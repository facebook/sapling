/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::Any;
use std::fmt::Debug;
use std::fs;
use std::io;
use std::path::Path;

use vlqencoding::VLQDecode;
use vlqencoding::VLQEncode;

use crate::errors::IoResultExt;
use crate::utils::atomic_write_plain;
use crate::utils::xxhash;
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
}
