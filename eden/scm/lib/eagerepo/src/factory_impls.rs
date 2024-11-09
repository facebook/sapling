/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Register factory constructors.

use std::sync::Arc;

use storemodel::SerializationFormat;
use storemodel::StoreInfo;
use storemodel::StoreOutput;

use crate::cas::cas_client_from_config;
use crate::EagerRepoStore;

pub(crate) fn init() {
    fn maybe_construct_eagerepo_store(
        info: &dyn StoreInfo,
    ) -> anyhow::Result<Option<Box<dyn StoreOutput>>> {
        if info.has_requirement("eagerepo")
            || (!info.has_requirement("git") && !info.has_requirement("remotefilelog"))
        {
            // Explicit and implicit eagerepo (not git and not remotefilelog -> revlog).
            // Revlog is now eagerepo + metadata. See D47878774.
            //
            // The pure Rust logic does not understand revlog but fine with eagerepo.
            // Note: The Python logic might still want to use the non-eager storage
            // like filescmstore etc.
            let store_path = info.store_path();
            let format = match info.has_requirement("git") {
                true => SerializationFormat::Git,
                false => SerializationFormat::Hg,
            };
            // The hgcommits/v1 path shares objects with commits.
            // Maybe it should be renamed to hg-objects.
            let store = EagerRepoStore::open(&store_path.join("hgcommits").join("v1"), format)?;
            let store = Arc::new(store);
            Ok(Some(Box::new(store) as Box<dyn StoreOutput>))
        } else {
            Ok(None)
        }
    }
    factory::register_constructor("eager", maybe_construct_eagerepo_store);

    factory::register_constructor("eager", cas_client_from_config);
}
