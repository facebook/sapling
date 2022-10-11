/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsuppported pattern kind {0}")]
    UnsupportedPatternKind(String),
}
