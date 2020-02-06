/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use stats::prelude::*;

define_stats! {
    prefix = "mononoke.bundle2_resolver";
    deltacache_dsize: histogram(400, 0, 100_000, Average, Sum, Count; P 50; P 95; P 99),
    deltacache_dsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    deltacache_fsize: histogram(400, 0, 100_000, Average, Sum, Count; P 50; P 95; P 99),
    deltacache_fsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    bookmark_pushkeys_count: timeseries(Rate, Average, Sum),
    changesets_count: timeseries(Rate, Average, Sum),
    manifests_count: timeseries(Rate, Average, Sum),
    filelogs_count: timeseries(Rate, Average, Sum),
    content_blobs_count: timeseries(Rate, Average, Sum),
    per_changeset_manifests_count: timeseries(Rate, Average, Sum),
    per_changeset_filelogs_count: timeseries(Rate, Average, Sum),
    per_changeset_content_blobs_count: timeseries(Rate, Average, Sum),
}
