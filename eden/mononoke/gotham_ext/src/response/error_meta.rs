/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::pin::Pin;

pub struct ErrorMeta<E> {
    /// Errors that were captured
    pub errors: Vec<E>,

    /// Extra erorrs that were observed but not captured.
    pub extra_error_count: u64,
}

impl<E> ErrorMeta<E> {
    pub fn new() -> Self {
        Self {
            errors: Vec::new(),
            extra_error_count: 0,
        }
    }

    pub fn one_error(e: E) -> Self {
        Self {
            errors: vec![e],
            extra_error_count: 0,
        }
    }

    pub fn error_count(&self) -> u64 {
        // NOTE: unwrap is safe since we if we have all those elements in a Vec, we're going to be
        // able to fit that into a u64.
        let n: u64 = self.errors.len().try_into().unwrap();
        n + self.extra_error_count
    }
}

pub trait ErrorMetaProvider<E> {
    fn report_errors(self: Pin<&mut Self>, error_meta: &mut ErrorMeta<E>);
}
