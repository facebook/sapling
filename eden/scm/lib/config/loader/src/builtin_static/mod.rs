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
use configset::config::ConfigSet;
use configset::config::Options;
use identity::Identity;
use staticconfig::static_config;
use staticconfig::StaticConfig;
use unionconfig::UnionConfig;

use crate::hg::OptionsHgExt;

mod core;
mod merge_tools;
pub(crate) mod sapling;

/// Return static builtin system config.
///
/// The actual selection of configs depends on `ident`.
///
/// This config is intended to have the lowest priority and can be overridden
/// by system config files.
pub(crate) fn builtin_system(opts: Options, ident: &Identity) -> UnionConfig {
    let mut configs: Vec<Arc<dyn Config>> = vec![Arc::new(&core::CONFIG)];
    if ident.env_var("CONFIG").is_none() {
        configs.push(Arc::new(&merge_tools::CONFIG));
    }
    let is_test = std::env::var_os("TESTTMP").is_some();
    let force_prod = std::env::var_os("TEST_PROD_CONFIGS").is_some();
    if ident.cli_name() == "sl" && (!is_test || force_prod) {
        configs.push(Arc::new(&sapling::CONFIG));
    }

    #[cfg(feature = "fb")]
    if !is_test || force_prod {
        configs.push(Arc::new(&crate::fb::static_system::CONFIG));
    }

    // Include a test static config so it's easy for unit and
    // integration tests to cover static config.
    if is_test {
        configs.push(Arc::new(&TEST_CONFIG));
    }

    apply_filters(UnionConfig::from_configs(configs), opts)
}

// Apply filter funcs from Options. This can do various things such as
// ignoring or renaming certain sections.
fn apply_filters(mut uc: UnionConfig, opts: Options) -> UnionConfig {
    let mut filter_overrides = ConfigSet::new();
    let opts = opts.source("builtin").process_hgplain();

    let filtered_opts: Options = "(filtered)".into();
    for section in uc.sections().iter() {
        for name in uc.keys(section) {
            let value = uc.get(section, &name);

            match opts.filter(section.clone(), name.clone(), value.clone()) {
                // None means this value was complete filtered out.
                // Insert explicit `None` into the overrides.
                None => filter_overrides.set(section, name, None::<&str>, &filtered_opts),
                Some((s, n, v)) => {
                    if s != section || n != name {
                        // If the filter mutated the section or name,
                        // first we need to add an override to hide
                        // the previous value.
                        filter_overrides.set(section, name, None::<&str>, &filtered_opts)
                    } else if v == value {
                        // If the config item is unchanged, we don't need an override.
                        continue;
                    }

                    filter_overrides.set(s, n, v, &filtered_opts)
                }
            }
        }
    }

    uc.push(Arc::new(filter_overrides));

    uc
}

/// Static system config used in tests.
pub static TEST_CONFIG: StaticConfig = static_config!("builtin:test_config" => r#"
[alias]
some-command = some-command --some-flag
"#);
