/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
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
