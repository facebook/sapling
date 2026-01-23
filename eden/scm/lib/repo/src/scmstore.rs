/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use revisionstore::SaplingRemoteApiFileStore;
use revisionstore::SaplingRemoteApiTreeStore;
use revisionstore::scmstore;
use revisionstore::scmstore::FileStoreBuilder;
use revisionstore::scmstore::TreeStoreBuilder;
use storemodel::SerializationFormat;
use storemodel::StoreInfo;

pub fn build_scm_file_store(info: &dyn StoreInfo) -> Result<Arc<scmstore::FileStore>> {
    tracing::trace!(target: "repo::file_store", "building filestore");
    let mut file_builder = FileStoreBuilder::new(info.config());

    if let Some(store_path) = info.store_path() {
        file_builder = file_builder.local_path(store_path);
    }

    if let Some(eden_api) = info.remote_peer()? {
        tracing::trace!(target: "repo::file_store", "enabling edenapi");
        file_builder = file_builder.edenapi(SaplingRemoteApiFileStore::new(eden_api));
    } else {
        tracing::trace!(target: "repo::file_store", "disabling edenapi");
        file_builder = file_builder.override_edenapi(false);
    }

    if info.has_requirement("git") {
        tracing::trace!(target: "repo::file_store", "enabling git serialization");
        file_builder = file_builder.format(SerializationFormat::Git);
    }

    tracing::trace!(target: "repo::file_store", "building file store");
    let file_store = file_builder.build().context("when building FileStore")?;

    Ok(Arc::new(file_store))
}

pub fn build_scm_tree_store(
    info: &dyn StoreInfo,
    file_store: Option<Arc<scmstore::FileStore>>,
) -> Result<Arc<scmstore::TreeStore>> {
    tracing::trace!(target: "repo::tree_store", "building treestore");
    let mut tree_builder = TreeStoreBuilder::new(info.config()).suffix("manifests");

    if let Some(store_path) = info.store_path() {
        tree_builder = tree_builder.local_path(store_path);
    }

    if let Some(eden_api) = info.remote_peer()? {
        tracing::trace!(target: "repo::tree_store", "enabling edenapi");
        tree_builder = tree_builder.edenapi(SaplingRemoteApiTreeStore::new(eden_api));
    } else {
        tracing::trace!(target: "repo::tree_store", "disabling edenapi");
        tree_builder = tree_builder.override_edenapi(false);
    }

    // The presence of the file store on the tree store causes the tree store to
    // request tree metadata (and write it back to file store aux cache).
    if let Some(file_store) = file_store {
        tracing::trace!(target: "repo::tree_store", "configuring filestore for aux fetching");
        tree_builder = tree_builder.filestore(file_store);
    } else {
        tracing::trace!(target: "repo::tree_store", "no filestore for aux fetching");
    }

    if info.has_requirement("git") {
        tracing::trace!(target: "repo::tree_store", "enabling git serialization");
        tree_builder = tree_builder.format(SerializationFormat::Git);
    }

    tracing::trace!(target: "repo::tree_store", "building tree store");
    let tree_store = tree_builder.build().context("when building TreeStore")?;

    Ok(Arc::new(tree_store))
}
