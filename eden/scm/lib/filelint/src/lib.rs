/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod materialize;

pub use materialize::ContentFingerprint;
pub use materialize::MaterializeFile;
pub use materialize::PrefilterResult;
pub use materialize::materialize_files;
pub use materialize::prefilter_files;
