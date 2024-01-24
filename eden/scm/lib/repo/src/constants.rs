/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashSet;

use once_cell::sync::Lazy;

pub static REPONAME_FILE: &str = "reponame";
pub static CHANGELOG_FILE: &str = "00changelog.i";
pub static REQUIREMENTS_FILE: &str = "requires";
pub static STORE_PATH: &str = "store";
pub static SUPPORTED_DEFAULT_REQUIREMENTS: Lazy<HashSet<String>> = Lazy::new(|| {
    HashSet::from([
        "eden".to_owned(),
        "revlogv1".to_owned(),
        "generaldelta".to_owned(),
        "store".to_owned(),
        "fncache".to_owned(),
        "shared".to_owned(),
        "relshared".to_owned(),
        "dotencode".to_owned(),
        "treestate".to_owned(),
        "storerequirements".to_owned(),
        "lfs".to_owned(),
        "globalrevs".to_owned(),
        "lz4revlog".to_owned(),
        "globalrevs".to_owned(),
        "windowssymlinks".to_owned(),
        "hgsql".to_owned(),
        "remotefilelog".to_owned(),
        // allows sparse eden (filteredfs) checkouts
        "edensparse".to_owned(),
    ])
});
pub static SUPPORTED_STORE_REQUIREMENTS: Lazy<HashSet<String>> = Lazy::new(|| {
    HashSet::from([
        "visibleheads".to_owned(),
        "narrowheads".to_owned(),
        "zstorecommitdata".to_owned(),
        "invalidatelinkrev".to_owned(),
        // python revlog
        "pythonrevlogchangelog".to_owned(),
        // rust revlog
        "rustrevlogchangelog".to_owned(),
        // pure segmented changelog (full idmap, full hgommits)
        "segmentedchangelog".to_owned(),
        // segmented changelog (full idmap, partial hgcommits) + revlog
        "doublewritechangelog".to_owned(),
        // hybrid changelog (full idmap, partial hgcommits) + revlog + edenapi
        "hybridchangelog".to_owned(),
        // use git format
        "git".to_owned(),
        // backed by git bare repo
        "git-store".to_owned(),
        // lazy commit message (full idmap, partial hgcommits) + edenapi
        "lazytextchangelog".to_owned(),
        // lazy commit message (sparse idmap, partial hgcommits) + edenapi
        "lazychangelog".to_owned(),
        // commit graph is truncated for emergency use-case. The first commit
        // has wrong parents.
        "emergencychangelog".to_owned(),
        // backed by Rust eagerepo::EagerRepo. Mainly used in tests or
        // fully local repos.
        "eagerepo".to_owned(),
    ])
});
