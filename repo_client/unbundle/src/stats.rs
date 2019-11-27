/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

pub use stats::{define_stats, prelude::*};

define_stats! {
    prefix = "mononoke.bundle2_resolver";
    deltacache_dsize: histogram(400, 0, 100_000, AVG, SUM, COUNT; P 50; P 95; P 99),
    deltacache_dsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    deltacache_fsize: histogram(400, 0, 100_000, AVG, SUM, COUNT; P 50; P 95; P 99),
    deltacache_fsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    bookmark_pushkeys_count: timeseries(RATE, AVG, SUM),
    changesets_count: timeseries(RATE, AVG, SUM),
    manifests_count: timeseries(RATE, AVG, SUM),
    filelogs_count: timeseries(RATE, AVG, SUM),
    content_blobs_count: timeseries(RATE, AVG, SUM),
    per_changeset_manifests_count: timeseries(RATE, AVG, SUM),
    per_changeset_filelogs_count: timeseries(RATE, AVG, SUM),
    per_changeset_content_blobs_count: timeseries(RATE, AVG, SUM),
}
