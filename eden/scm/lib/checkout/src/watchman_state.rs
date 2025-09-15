/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use anyhow::bail;
use configmodel::ConfigExt;
use edenfs_asserted_states_client::AssertedStatesClient;
use edenfs_asserted_states_client::ContentLockGuard;
use tracing::instrument;
use types::HgId;
use watchman_client::Value;
use watchman_client::prelude::*;
use workingcopy::filesystem::FileSystemType;
use workingcopy::workingcopy::LockedWorkingCopy;

#[derive(Default)]
pub struct WatchmanStateChange {
    client: Option<(Arc<Client>, ResolvedRoot)>,
    target: HgId,
    success: bool,
    #[allow(dead_code)] // This object hold the lock, and does not need to be accessed
    lock: Option<ContentLockGuard>,
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

            let mut lock = None;
            if cfg!(feature = "eden")
                && wc
                    .config()
                    .get_or("experimental", "enable-edenfs-asserted-states", || false)?
                && wc.file_system_type == FileSystemType::Eden
            {
                let asserted_states_client = AssertedStatesClient::new(wc.vfs().root())?;
                let content_lock_guard = asserted_states_client.enter_state_with_deadline(
                    "hg.update",
                    wc.config()
                        .get_or::<Duration>("edenfs", "eden-state-timeout", || {
                            Duration::from_secs(1)
                        })?,
                    wc.config()
                        .get_or::<Duration>("devel", "lock_backoff", || {
                            Duration::from_secs_f64(0.1)
                        })?,
                );
                // (T237640498) Currently we have watchman propagate its error, and edenfs state lock just logs.
                // Long term we'll want to switch these
                match content_lock_guard {
                    Ok(guard) => lock = Some(guard),
                    Err(err) => {
                        tracing::error!("failed to acquire edenfs content lock: {:?}", err);
                    }
                }
            }

            Ok(Self {
                client: Some((client, root)),
                target,
                lock,
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

        // Dropping the object implicitly drops the eden notifications lock
    }
}
