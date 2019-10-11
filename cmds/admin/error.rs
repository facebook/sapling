/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use failure_ext::Error;

pub enum SubcommandError {
    InvalidArgs,
    Error(Error),
}

impl From<Error> for SubcommandError {
    fn from(err: Error) -> SubcommandError {
        SubcommandError::Error(err)
    }
}
