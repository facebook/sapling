/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Config loaded only for the Sapling identity.
/// This config contains behavior changes when running "sl" vs "hg", so normally this is
/// _not_ where you want to add default config values.
pub static CONFIG: StaticConfig = static_config!("builtin:sapling" => r###"
[remotefilelog]
# Internally this will be overridden by dynamic config to be ~/.hgcache.
cachepath=~/.sl_cache
"###);
