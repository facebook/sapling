/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use thiserror::Error;

#[derive(Debug, Error)]
pub enum ErrorKind {
    #[error("Injected failure in get to ChaosBlobstore for key {0}")]
    InjectedChaosGet(String),
    #[error("Injected failure in put to ChaosBlobstore for key {0}")]
    InjectedChaosPut(String),
    #[error("Injected failure in is_present to ChaosBlobstore for key {0}")]
    InjectedChaosIsPresent(String),
}
