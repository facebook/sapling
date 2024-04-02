/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;

mod acl_regions;
mod commit_sync;
mod common;
pub(crate) mod repo;
mod storage;

/// Trait for converting raw config into parsed config.
pub trait Convert {
    /// Conversion target
    type Output;

    /// Try to convert `self` into `Self::Output`
    fn convert(self) -> Result<Self::Output>;
}

impl<T: Convert> Convert for Option<T> {
    type Output = Option<<T as Convert>::Output>;

    fn convert(self) -> Result<Self::Output> {
        match self {
            Some(v) => Ok(Some(v.convert()?)),
            None => Ok(None),
        }
    }
}

impl<T: Convert> Convert for Vec<T> {
    type Output = Vec<<T as Convert>::Output>;

    fn convert(self) -> Result<Self::Output> {
        self.into_iter()
            .map(<T as Convert>::convert)
            .collect::<Result<_>>()
    }
}
