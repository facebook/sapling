/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug subscribe

#[cfg(unix)]
use std::ffi::OsStr;
#[cfg(unix)]
use std::os::unix::ffi::OsStringExt;
#[cfg(unix)]
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use std::time::Instant;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::EdenFsInstance;
use futures::StreamExt;
use serde::Serialize;
use thrift_types::edenfs as edenfs_thrift;
use tokio::io::AsyncWriteExt;
use tokio::sync::Notify;

use crate::util::expand_path_or_cwd;
use crate::util::jsonrpc::ResponseBuilder;
use crate::util::locate_repo_root;
use crate::ExitCode;

#[derive(Debug, Serialize)]
struct SubscribeResponse {
    mount_generation: i64,
    // Thrift somehow generates i64 for unsigned64 type
    sequence_number: i64,
    snapshot_hash: String,
}

impl From<edenfs_thrift::JournalPosition> for SubscribeResponse {
    fn from(from: edenfs_thrift::JournalPosition) -> Self {
        Self {
            mount_generation: from.mountGeneration,
            sequence_number: from.sequenceNumber,
            snapshot_hash: hex::encode(from.snapshotHash),
        }
    }
}

#[derive(Parser, Debug)]
#[clap(about = "Subscribes to journal changes. Responses are in JSON format")]
pub struct SubscribeCmd {
    #[clap(parse(try_from_str = expand_path_or_cwd), default_value = "")]
    /// Path to the mount point
    mount_point: PathBuf,

    #[clap(short, long, default_value = "500")]
    /// [Unit: ms] number of milliseconds to wait between events
    throttle: u64,
}

impl SubscribeCmd {
    async fn _make_notify_event(mount_point: &Vec<u8>) -> ResponseBuilder {
        let instance = EdenFsInstance::global();
        let client = match instance.connect(None).await {
            Ok(client) => client,
            Err(e) => {
                return ResponseBuilder::error(&format!(
                    "error while establishing connection to EdenFS server {e:?}"
                ));
            }
        };

        let journal = match client.getCurrentJournalPosition(mount_point).await {
            Ok(journal) => journal,
            Err(e) => {
                return ResponseBuilder::error(&format!(
                    "error while getting current journal position: {e:?}",
                ));
            }
        };

        match serde_json::to_value(SubscribeResponse::from(journal)) {
            Err(e) => ResponseBuilder::error(&format!(
                "error while serializing subscription response: {e:?}",
            )),
            Ok(serialized) => ResponseBuilder::result(serialized),
        }
    }
}

#[async_trait]
impl crate::Subcommand for SubscribeCmd {
    #[cfg(not(fbcode_build))]
    async fn run(&self) -> Result<ExitCode> {
        eprintln!("not supported in non-fbcode build");
        Ok(1)
    }

    #[cfg(fbcode_build)]
    async fn run(&self) -> Result<ExitCode> {
        let mount_point_path = locate_repo_root(&self.mount_point)
            .with_context(|| anyhow!("unable to locate repository root"))?;
        #[cfg(unix)]
        let mount_point = <Path as AsRef<OsStr>>::as_ref(mount_point_path)
            .to_os_string()
            .into_vec();
        // SAFETY: paths on Windows are Unicode
        #[cfg(windows)]
        let mount_point = mount_point_path.to_string_lossy().into_owned().into_bytes();
        let stream_client = EdenFsInstance::global()
            .connect_streaming(None)
            .await
            .with_context(|| anyhow!("unable to establish Thrift connection to EdenFS server"))?;

        let notify = Arc::new(Notify::new());

        tokio::task::spawn({
            let notify = notify.clone();
            let mount_point = mount_point.clone();

            async move {
                let mut stdout = tokio::io::stdout();
                loop {
                    notify.notified().await;
                    let response = Self::_make_notify_event(&mount_point).await.build();

                    match serde_json::to_vec(&response) {
                        Ok(mut bytes) => {
                            bytes.push(b'\n');
                            stdout.write_all(&bytes).await.ok();
                        }
                        Err(e) => {
                            tracing::error!(?e, ?response, "unable to seralize response to JSON");
                        }
                    }
                }
            }
        });

        // TODO: feels weird that this method accepts a `&Vec<u8>` instead of a `&[u8]`.
        let mut subscription = stream_client.subscribeStreamTemporary(&mount_point).await?;
        tracing::info!(?mount_point_path, "subscription created");

        let mut last = Instant::now();
        let throttle = Duration::from_millis(self.throttle);
        while let Some(journal) = subscription.next().await {
            match journal {
                Ok(_) => {
                    if last.elapsed() >= throttle {
                        notify.notify_one();
                        last = Instant::now();
                    }
                }
                Err(e) => tracing::error!(?e, "error while processing subscription"),
            }
        }

        Ok(0)
    }
}
