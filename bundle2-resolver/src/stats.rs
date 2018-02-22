// Copyright (c) 2004-present, Facebook, Inc.
// All Rights Reserved.
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

pub use stats_crate::prelude::*;

define_stats! {
    prefix = "mononoke.bundle2_resolver";
    deltacache_dsize: histogram(400, 0, 100_000, AVG, SUM, COUNT; P 50; P 95; P 99),
    deltacache_dsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    deltacache_fsize: histogram(400, 0, 100_000, AVG, SUM, COUNT; P 50; P 95; P 99),
    deltacache_fsize_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
    deltacache_size: histogram(400, 0, 100_000, AVG, SUM, COUNT; P 50; P 95; P 99),
    deltacache_size_large: histogram(400_000, 0, 100_000_000; P 50; P 95; P 99),
}
