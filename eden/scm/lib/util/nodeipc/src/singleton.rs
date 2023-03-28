/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use once_cell::sync::Lazy;

use crate::NodeIpc;

/// [`NodeIpc`] initialized from the environment variable on demand.
///
/// See [`NodeIpc::from_env`] for details. Accessing this state for
/// the first time might have side effects on environment variables.
/// So it's recommended to access this before creating threads.
pub static IPC: Lazy<Option<NodeIpc>> = Lazy::new(NodeIpc::from_env);
