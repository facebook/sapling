/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Static defaults loaded only for EdenFS working copies. Overrides other static configs.
///
/// Windows keeps `abort-on-eden-conflict-error` off: files held open by other
/// processes routinely make EdenFS report a removal failure there, and aborting
/// those checkouts would regress the warning that non-EdenFS checkout prints.
#[cfg(not(windows))]
pub static EDEN_CONFIG: StaticConfig = static_config!("builtin:eden" => r###"
[experimental]
abort-on-eden-conflict-error = true
abort-on-eden-directory-conflict = true

[fsmonitor]
timeout = 1
"###);

#[cfg(windows)]
pub static EDEN_CONFIG: StaticConfig = static_config!("builtin:eden" => r###"
[experimental]
abort-on-eden-directory-conflict = true

[fsmonitor]
timeout = 1
"###);
