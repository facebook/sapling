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

    #[error("{0} not under root '{1}'")]
    PathOutsideRoot(String, String),

    #[error("non-utf8 path '{0}' when building pattern")]
    NonUtf8(String),

    #[error("listfile:- may only be used once as a direct CLI argument")]
    StdinUnavailable,
}
