/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Repo tool compatibility
//!
//! This crate provides capabilities to support the
//! [repo tool](https://gerrit.googlesource.com/git-repo) with `.repo/` identity.

pub mod commit;
pub mod init;
pub mod path;
pub mod trees;
