/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
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
