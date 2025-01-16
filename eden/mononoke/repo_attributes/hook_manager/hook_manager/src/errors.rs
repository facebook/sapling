/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum HookManagerError {
    #[error("No such hook '{0}'")]
    NoSuchHook(String),
}
