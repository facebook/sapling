/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;

pub enum SubcommandError {
    InvalidArgs,
    Error(Error),
}

impl From<Error> for SubcommandError {
    fn from(err: Error) -> SubcommandError {
        SubcommandError::Error(err)
    }
}
