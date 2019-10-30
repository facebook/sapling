/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use blobrepo::BlobRepo;
use bonsai_globalrev_mapping::{BonsaiGlobalrevMapping, BonsaiGlobalrevMappingEntry};
use failure_ext::Error;
use futures_ext::{BoxFuture, FutureExt};
use mercurial_types::Globalrev;
use mononoke_types::BonsaiChangeset;
use std::sync::Arc;

pub fn upload_globalrevs(
    repo: BlobRepo,
    globalrevs_store: Arc<dyn BonsaiGlobalrevMapping>,
    cs_ids: Vec<BonsaiChangeset>,
) -> BoxFuture<(), Error> {
    let repo_id = repo.get_repoid();
    let mut entries = vec![];
    for bcs in cs_ids {
        let global_rev = Globalrev::from_bcs(bcs.clone());
        if let Ok(globalrev) = global_rev {
            let entry =
                BonsaiGlobalrevMappingEntry::new(repo_id, bcs.get_changeset_id(), globalrev);
            entries.push(entry);
        }
    }
    globalrevs_store.add_many(entries).boxify()
}
