/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Statically _compiled_ configs. See `core` or `merge_tools` for example.
//!
//! Use `staticconfig::static_config!` to define static configs so they do not
//! have runtime parsing or hashmap insertion overhead.

use std::sync::Arc;

use configmodel::Config;
use identity::Identity;
use unionconfig::UnionConfig;

mod core;
mod merge_tools;

/// Return static builtin system config.
///
/// The actual selection of configs depends on `ident`.
///
/// This config is intended to have the lowest priority and can be overridden
/// by system config files.
pub(crate) fn builtin_system(ident: &Identity) -> UnionConfig {
    let mut configs: Vec<Arc<dyn Config>> = vec![Arc::new(&core::CONFIG)];
    if ident.env_var("CONFIG").is_none() {
        configs.push(Arc::new(&merge_tools::CONFIG));
    }
    UnionConfig::from_configs(configs)
}
