/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */
use crate::EagerRepoStore;
use cpython::*;
use cpython_ext::convert::register_into;
use std::sync::Arc;
use storemodel::ReadFileContents;
use storemodel::TreeStore;

pub(crate) fn register(py: Python) {
    register_into(py, |py, obj: EagerRepoStore| obj.to_dyn_treestore(py));
    register_into(py, |py, obj: EagerRepoStore| obj.to_read_file_contents(py));
}

impl EagerRepoStore {
    fn to_dyn_treestore(&self, py: Python) -> Arc<dyn TreeStore + Send + Sync> {
        let store = self.inner(py);
        Arc::new(store.clone())
    }

    fn to_read_file_contents(
        &self,
        py: Python,
    ) -> Arc<dyn ReadFileContents<Error = anyhow::Error> + Send + Sync> {
        let store = self.inner(py).clone();
        Arc::new(store)
    }
}
