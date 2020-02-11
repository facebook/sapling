/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("No changeset with id '{0}'")]
    NoSuchChangeset(String),
}
