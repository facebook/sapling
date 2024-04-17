/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]
pub mod sql;
pub mod workspace;
use crate::sql::ops::SqlCommitCloud;
#[facet::facet]
pub struct CommitCloud {
    pub storage: SqlCommitCloud,
}
