/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::Result;
use cpython::Python;
use cpython_ext::convert::register_into;
use manifest_tree::TreeResolver;

use crate::repo;

pub(crate) fn register(py: Python) {
    register_into(py, |py, repo: repo| -> Result<Arc<dyn TreeResolver>> {
        repo.read_repo(py)
            .tree_resolver()
            .map(|resolver| resolver as Arc<dyn TreeResolver>)
    });
}
