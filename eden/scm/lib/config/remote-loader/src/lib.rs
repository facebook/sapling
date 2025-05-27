/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(feature = "fb")]
mod fb;
#[cfg(feature = "fb")]
pub use fb::remote_loader::get_remote_configs;
#[cfg(feature = "fb")]
pub use fb::remote_loader::maybe_set_http_cat_header;
