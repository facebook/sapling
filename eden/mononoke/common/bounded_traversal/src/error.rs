/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum BoundedTraversalError {
    #[error("Programming error at {file}:{line}: {desc}")]
    ProgrammingError {
        desc: String,
        file: &'static str,
        line: u32,
    },
}

macro_rules! programming_error {
    ( $( $args:tt )* ) => {
        $crate::error::BoundedTraversalError::ProgrammingError {
            desc: format!( $( $args )* ),
            file: file!(),
            line: line!(),
        }
    };
}
