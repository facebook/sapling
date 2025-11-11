/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;

use dag::protocol::RemoteIdConvertProtocol;
use eagerepo_trait::EagerRepoExtension;
use eagerepo_trait::Id20StoreExtension;
use minibytes::Bytes;
use types::Id20;
use types::SerializationFormat;

use crate::dag_protocol::VirtualIdConvertProtocol;
use crate::provider::VirtualRepoProvider;

impl Id20StoreExtension for VirtualRepoProvider {
    fn get_sha1_blob(&self, id: Id20) -> Option<Bytes> {
        VirtualRepoProvider::get_sha1_blob(self, id)
    }

    fn get_content(&self, id: Id20) -> Option<Bytes> {
        VirtualRepoProvider::get_content(self, id)
    }

    fn format(&self) -> SerializationFormat {
        self.format
    }

    fn name(&self) -> &'static str {
        "virtual-repo"
    }
}

impl EagerRepoExtension for VirtualRepoProvider {
    fn get_dag_remote_protocol(&self) -> Option<Arc<dyn RemoteIdConvertProtocol>> {
        Some(Arc::new(VirtualIdConvertProtocol))
    }

    fn name(&self) -> &'static str {
        "virtual-repo"
    }
}
