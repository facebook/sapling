/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#[cfg(fbcode_build)]
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
#[cfg(fbcode_build)]
use slog::info;
use slog::Logger;
#[cfg(fbcode_build)]
use zkserverguard_helper::become_leader;
#[cfg(fbcode_build)]
use zkserverguard_helper::ServerGuard;
#[cfg(fbcode_build)]
use zkserverguard_helper::ZkServerGuardTier;

#[cfg(fbcode_build)]
const ZEUS_CLIENT_ID: &str = "mononoke";
#[derive(PartialEq, Clone, Copy, Debug)]
pub enum ZkMode {
    /// A leader coordinated using ZkServerGuard (prod) except OSS
    Enabled,
    /// Leader election disabled (for tests)
    Disabled,
}

impl From<bool> for ZkMode {
    fn from(f: bool) -> ZkMode {
        match f {
            true => ZkMode::Enabled,
            false => ZkMode::Disabled,
        }
    }
}

#[cfg(fbcode_build)]
fn shutdown() {
    eprintln!(
        "ERROR: Process lost leadership somehow! Terminating process to avoid multiple leaders",
    );
    std::process::abort();
}

#[async_trait]
pub trait LeaderElection {
    fn get_shared_lock_path(&self) -> String;

    #[cfg(not(fbcode_build))]
    async fn maybe_become_leader(&self, _mode: ZkMode, _logger: Logger) -> Result<Option<()>> {
        Ok(None)
    }

    #[cfg(fbcode_build)]
    async fn maybe_become_leader(
        &self,
        mode: ZkMode,
        logger: Logger,
    ) -> Result<Option<ServerGuard>> {
        match mode {
            ZkMode::Enabled => {
                let path = self.get_shared_lock_path();
                info!(
                    logger,
                    "Waiting for lock on {} for ZkServerGuard prod tier", path
                );
                Ok(Some(
                    become_leader(
                        ZEUS_CLIENT_ID.to_string(),
                        ZkServerGuardTier::Prod,
                        path.clone(),
                        shutdown,
                    )
                    .await
                    .with_context(|| {
                        format!(
                            "Failed to become leader using lock {} against prod ZkServerGuard",
                            path
                        )
                    })?,
                ))
            }
            ZkMode::Disabled => Ok(None),
        }
    }
}
