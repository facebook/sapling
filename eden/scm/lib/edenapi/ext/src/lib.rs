/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This module has auto-implemented functions that expand on SaplingRemoteAPI functionalities.
// Think of it as a SaplingRemoteAPIExt trait full of auto-implemented functions.
// It's not implemented like that because trait implementations can't be split in
// multiple files, so this is instead implemented as many functions in different files.
// Always use the format:
// fn my_function(api: &(impl SaplingRemoteApi + ?Sized), other_args...) -> ... {...}
// this way the function can be called from inside any trait that extends SaplingRemoteAPI.

mod files;
mod snapshot;
mod snapshot_cache;
mod util;

pub use files::check_files;
pub use files::download_files_with_cache;
pub use snapshot::DownloadFileStats;
pub use snapshot::DownloadFileStatsSnapshot;
pub use snapshot::fetch_snapshot_with_cache;
pub use snapshot::upload_snapshot;
pub use snapshot::upload_snapshot_with_cache;
pub use snapshot_cache::SharedSnapshotFileCache;
pub use snapshot_cache::SnapshotFileCache;
pub use util::calc_contentid;
