/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! edenfsctl debug subscribe

use std::path::PathBuf;

use anyhow::Result;
use async_trait::async_trait;
use clap::Parser;
use edenfs_client::changes_since::ChangesSinceV2Result;
use edenfs_client::types::JournalPosition;
use edenfs_client::utils::get_mount_point;
use edenfs_error::EdenFsError;
use futures::StreamExt;
use hg_util::path::expand_path;
use serde::Serialize;
use tokio::io::AsyncWriteExt;

use crate::ExitCode;
use crate::get_edenfs_instance;
use crate::util::jsonrpc::ResponseBuilder;

// Defines a few helper functions to make the debug format easier to read.
mod fmt {
    use std::fmt;
    use std::fmt::Debug;

    use edenfs_client::changes_since::ChangeNotification;
    use edenfs_client::changes_since::ChangesSinceV2Result;
    use edenfs_client::types::JournalPosition;
    use thrift_types::edenfs as edenfs_thrift;

    /// Courtesy of https://users.rust-lang.org/t/reusing-an-fmt-formatter/8531/4
    ///
    /// This allows us to provide customized format implementation to avoid
    /// using the default one.
    pub struct Fmt<F>(pub F)
    where
        F: Fn(&mut fmt::Formatter) -> fmt::Result;

    impl<F> fmt::Debug for Fmt<F>
    where
        F: Fn(&mut fmt::Formatter) -> fmt::Result,
    {
        fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
            (self.0)(f)
        }
    }

    fn debug_hash(hash: &edenfs_thrift::ThriftRootId) -> impl Debug + '_ {
        Fmt(move |f| write!(f, "{}", hex::encode(hash)))
    }

    fn debug_position(position: &JournalPosition) -> impl Debug + '_ {
        Fmt(|f| {
            f.debug_struct("JournalPosition")
                .field("mountGeneration", &position.mount_generation)
                .field("sequenceNumber", &position.sequence_number)
                .field("snapshotHash", &debug_hash(&position.snapshot_hash))
                .finish()
        })
    }

    pub fn debug_change_notification(notification: &ChangeNotification) -> impl Debug + '_ {
        let notification_str = notification.to_string();
        Fmt(move |f| write!(f, "{}", notification_str))
    }

    pub fn debug_changes_since_result(result: &ChangesSinceV2Result) -> impl Debug + '_ {
        Fmt(|f| {
            f.debug_struct("ChangesSinceV2Result")
                .field("toPosition", &debug_position(&result.to_position))
                .field(
                    "changes",
                    &Fmt(|f| {
                        f.debug_list()
                            .entries(result.changes.iter().map(debug_change_notification))
                            .finish()
                    }),
                )
                .finish()
        })
    }
}

#[derive(Debug, Serialize)]
struct SubscribeResponse {
    mount_generation: i64,
    sequence_number: u64,
    snapshot_hash: String,
}

impl From<JournalPosition> for SubscribeResponse {
    fn from(from: JournalPosition) -> Self {
        Self {
            mount_generation: from.mount_generation,
            sequence_number: from.sequence_number,
            snapshot_hash: hex::encode(from.snapshot_hash),
        }
    }
}

#[derive(Parser, Debug)]
#[clap(about = "Subscribes to journal changes. Responses are in JSON format")]
pub struct SubscribeCmd {
    #[clap(parse(from_str = expand_path))]
    /// Path to the mount point
    mount_point: Option<PathBuf>,

    #[clap(short, long, default_value = "500")]
    /// [Unit: ms] number of milliseconds to wait between events
    throttle: u64,
}

fn handle_result(result: &ChangesSinceV2Result) -> Result<(), EdenFsError> {
    tracing::debug!(changes = ?fmt::debug_changes_since_result(result));

    let response =
        match serde_json::to_value(SubscribeResponse::from(result.to_position.clone())) {
            Err(e) => ResponseBuilder::error(&format!(
                "error while serializing subscription response: {e:?}",
            )),
            Ok(serialized) => ResponseBuilder::result(serialized),
        }
        .build();

    match serde_json::to_string(&response) {
        Ok(string) => {
            println!("{}", string);
        }
        Err(e) => {
            tracing::error!(?e, ?response, "unable to serialize response to JSON");
        }
    }

    Ok(())
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
        let instance = get_edenfs_instance();
        let client = instance.get_client();

        let mount_point_path = get_mount_point(&self.mount_point)?;
        let position = client.get_journal_position(&self.mount_point).await?;

        let mut stdout = tokio::io::stdout();

        let response = ResponseBuilder::result(serde_json::json!({
            "message": format!("subscribed to {}", mount_point_path.display())
        }))
        .build();
        let mut bytes = serde_json::to_vec(&response).unwrap();
        bytes.push(b'\n');
        stdout.write_all(&bytes).await.ok();

        let stream = client
            .stream_changes_since(
                &self.mount_point,
                self.throttle,
                position,
                &None,
                &None,
                &None,
                &None,
                &None,
                false,
                false,
            )
            .await?;

        stream
            .for_each(|result| async move {
                match result {
                    Ok(result) => handle_result(&result).expect("Error while handling result."),
                    Err(e) => eprintln!("Error: {}", e),
                }
            })
            .await;

        Ok(0)
    }
}
