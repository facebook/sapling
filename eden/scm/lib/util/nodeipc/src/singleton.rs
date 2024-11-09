/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::RwLock;

use crate::NodeIpc;

// None: Not initialized yet.
// Some(None): Initialized and got None.
// Some(Some(...)): Initialized and has the value.
pub(crate) static IPC: RwLock<Option<Option<Arc<NodeIpc>>>> = RwLock::new(None);

/// [`NodeIpc`] initialized from the environment variable on demand.
///
/// See [`NodeIpc::from_env`] for details. Accessing this state for
/// the first time might have side effects on environment variables.
/// So it's recommended to access this before creating threads.
pub fn get_singleton() -> Option<Arc<NodeIpc>> {
    let ipc = IPC.read().unwrap();
    if let Some(ref ipc) = *ipc {
        return ipc.clone();
    }
    drop(ipc);

    let mut ipc = IPC.write().unwrap();
    if let Some(ref ipc) = *ipc {
        return ipc.clone();
    }
    let new_ipc = NodeIpc::from_env().map(Arc::new);
    *ipc = Some(new_ipc.clone());
    new_ipc
}
