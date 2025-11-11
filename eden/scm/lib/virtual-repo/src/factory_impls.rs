/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Register factory constructors.

use std::sync::Arc;

use eagerepo_trait::EagerRepoExtension;
use eagerepo_trait::Id20StoreExtension;
use types::SerializationFormat;

use crate::VirtualRepoProvider;

pub(crate) fn init() {
    fn maybe_provide_virtual_repo_extension<T>(
        info: &(String, SerializationFormat),
        convert_func: fn(VirtualRepoProvider) -> T,
    ) -> anyhow::Result<Option<T>> {
        let (name, format) = info;
        // NOTE: Perhaps factory can provide a way to register by string keys.
        if name == "virtual-repo" {
            let provider = VirtualRepoProvider::new(*format);
            let ext = convert_func(provider);
            Ok(Some(ext))
        } else {
            Ok(None)
        }
    }

    factory::register_constructor("virtual-repo", |info| {
        maybe_provide_virtual_repo_extension(info, |p| Arc::new(p) as Arc<dyn Id20StoreExtension>)
    });
    factory::register_constructor("virtual-repo", |info| {
        maybe_provide_virtual_repo_extension(info, |p| Arc::new(p) as Arc<dyn EagerRepoExtension>)
    });
}
