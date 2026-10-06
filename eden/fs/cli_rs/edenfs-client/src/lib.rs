/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! The full-featured EdenFS client library: the thriftclient-backed
//! [`client::EdenFsClient`] plus everything layered on top of it. The
//! daemon-independent instance/checkout/redirection layer lives in
//! `edenfs-core` and is re-exported here (with the client-backed extensions
//! merged in) so existing `edenfs_client::...` paths keep working.

pub mod attributes;
pub mod backing_store;
pub mod changes_since;
pub mod client;
pub mod config;
pub mod counter_names;
pub mod counters;
pub mod current_snapshot;
pub mod daemon;
pub mod daemon_info;
pub mod file_access_monitor;
pub mod glob_files;
pub mod instance;
pub mod journal;
pub mod methods;
pub mod prefetch_files;
mod prefetch_profiles_ext;
pub mod readdir;
mod redirect_add;
pub mod request_factory;
pub mod scm_status;
pub mod stats;
mod thrift_daemon;
pub mod types;
pub mod unmount;
pub mod use_case;

pub mod checkout {
    pub use edenfs_core::checkout::*;

    pub use crate::prefetch_profiles_ext::CheckoutPrefetchExt;
}

pub mod fsutil {
    pub use edenfs_core::fsutil::*;
}

pub mod redirect {
    pub use edenfs_core::redirect::*;

    pub use crate::redirect_add::try_add_redirection;
}

pub mod utils {
    pub use edenfs_core::utils::*;
}
