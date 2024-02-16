/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::bail;
use anyhow::Result;
use tracing::instrument;
use types::HgId;
use watchman_client::prelude::*;
use watchman_client::Value;
use workingcopy::workingcopy::LockedWorkingCopy;

#[derive(Default)]
pub struct WatchmanStateChange {
    client: Option<(Arc<Client>, ResolvedRoot)>,
    target: HgId,
    success: bool,
}

impl WatchmanStateChange {
    pub fn maybe_open(wc: &LockedWorkingCopy, target: HgId) -> Self {
        let span = tracing::info_span!("maybe_open", err = tracing::field::Empty);
        let _enter = span.enter();

        (|| -> Result<Self> {
            if matches!(
                wc.config().get("extensions", "hgevents").as_deref(),
                None | Some("!")
            ) {
                bail!("hgevents disabled");
            }

            let client = wc.watchman_client()?;

            let root = async_runtime::block_on(
                client.resolve_root(CanonicalPath::canonicalize(wc.vfs().root())?),
            )?;

            let metadata: HashMap<String, Value> = HashMap::from([
                ("status".to_string(), "ok".into()),
                ("rev".to_string(), wc.first_parent()?.to_hex().into()),
                // Is this important to calculate?
                ("distance".to_string(), 0i64.into()),
                ("merge".to_string(), false.into()),
                ("partial".to_string(), false.into()),
            ]);

            async_runtime::block_on(client.state_enter(
                &root,
                "hg.update",
                SyncTimeout::Duration(Duration::from_secs(1)),
                Some(metadata.into()),
            ))?;

            Ok(Self {
                client: Some((client, root)),
                target,
                ..Default::default()
            })
        })()
        .unwrap_or_else(|err| {
            span.record("err", format!("{:?}", err));
            Self::default()
        })
    }

    pub fn mark_success(&mut self) {
        self.success = true;
    }
}

impl Drop for WatchmanStateChange {
    #[instrument(skip_all)]
    fn drop(&mut self) {
        let (client, root) = match self.client.take() {
            None => return,
            Some((client, root)) => (client, root),
        };

        let metadata: HashMap<String, Value> = HashMap::from([
            (
                "status".to_string(),
                if self.success { "ok" } else { "failed" }.into(),
            ),
            ("rev".to_string(), self.target.to_hex().into()),
            ("distance".to_string(), 0i64.into()),
            ("merge".to_string(), false.into()),
            ("partial".to_string(), false.into()),
        ]);

        let _ = async_runtime::block_on(client.state_leave(
            &root,
            "hg.update",
            SyncTimeout::Duration(Duration::from_secs(1)),
            Some(metadata.into()),
        ));
    }
}
