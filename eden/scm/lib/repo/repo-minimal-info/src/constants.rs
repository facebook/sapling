/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use phf::phf_set;

pub static SUPPORTED_DEFAULT_REQUIREMENTS: phf::Set<&str> = phf_set! {
    "eden",
    "revlogv1",
    "generaldelta",
    "store",
    "fncache",
    "shared",
    "relshared",
    "dotencode",
    "treestate",
    "storerequirements",
    "lfs",
    "lz4revlog",
    "globalrevs",
    "windowssymlinks",
    "hgsql",
    "remotefilelog",
    // allows sparse eden (filteredfs) checkouts
    "edensparse",
    // repo is also a .git/ repo
    "dotgit",
};
pub static SUPPORTED_STORE_REQUIREMENTS: phf::Set<&str> = phf_set! {
    "visibleheads",
    "narrowheads",
    "zstorecommitdata",
    "invalidatelinkrev",
    // python revlog
    "pythonrevlogchangelog",
    // rust revlog
    "rustrevlogchangelog",
    // pure segmented changelog (full idmap, full hgommits)
    "segmentedchangelog",
    // segmented changelog (full idmap, partial hgcommits) + revlog
    "doublewritechangelog",
    // hybrid changelog (full idmap, partial hgcommits) + revlog + edenapi
    "hybridchangelog",
    // use git format
    "git",
    // backed by git bare repo
    "git-store",
    // lazy commit message (full idmap, partial hgcommits) + edenapi
    "lazytextchangelog",
    // lazy commit message (sparse idmap, partial hgcommits) + edenapi
    "lazychangelog",
    // commit graph is truncated for emergency use-case. The first commit
    // has wrong parents.
    "emergencychangelog",
    // backed by Rust eagerepo::EagerRepo. Mainly used in tests or
    // fully local repos.
    "eagerepo",
    // explicit requirement for a revlog repo using eager store (i.e. revlog2.py)
    "eagercompat",
    // repo is also a .git/ repo
    "dotgit",
};
