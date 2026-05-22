/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use staticconfig::StaticConfig;
use staticconfig::static_config;

/// Static defaults loaded only for EdenFS working copies. Overrides other static configs.
pub static EDEN_CONFIG: StaticConfig = static_config!("builtin:eden" => r###"
[fsmonitor]
timeout = 1
"###);
