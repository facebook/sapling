/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::any::Any;
use std::fmt::Debug;
use std::io;

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
