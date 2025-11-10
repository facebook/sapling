/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Register factory constructors.

use std::sync::Arc;

use eagerepo_trait::EagerRepoExtension;
use types::SerializationFormat;

use crate::VirtualRepoProvider;

pub(crate) fn init() {
    fn maybe_provide_virtual_repo_extension(
        info: &(String, SerializationFormat),
    ) -> anyhow::Result<Option<Arc<dyn EagerRepoExtension>>> {
        let (name, format) = info;
        // NOTE: Perhaps factory can provide a way to register by string keys.
        if name == "virtual-repo" {
            let provider = VirtualRepoProvider::new(*format);
            let ext = Arc::new(provider);
            Ok(Some(ext))
        } else {
            Ok(None)
        }
    }
    factory::register_constructor("virtual-repo", maybe_provide_virtual_repo_extension);
}
