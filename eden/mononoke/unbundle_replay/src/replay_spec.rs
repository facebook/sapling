/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::{format_err, Error};
use blobrepo::BlobRepo;
use blobstore::Loadable;
use bookmarks::{BookmarkName, BookmarkUpdateLogEntry};
use bytes::Bytes;
use context::CoreContext;
use futures::compat::Future01CompatExt;
use mercurial_types::HgChangesetId;
use mononoke_types::{hash::Blake2, ChangesetId, RawBundle2Id, Timestamp};
use slog::info;
use std::collections::HashMap;
use std::str::FromStr;
use std::time::Duration;
use tokio::process::Command;

use crate::hg_recording::HgRecordingEntry;
use crate::hooks::Target;

pub struct ExternalHandle<'a> {
    bundle_helper: &'a str,
    handle: String,
}

impl<'a> ExternalHandle<'a> {
    async fn load(&self) -> Result<Bytes, Error> {
        let output = Command::new(self.bundle_helper)
            .arg(&self.handle)
            .output()
            .await?;

        if !output.status.success() {
            let e = format_err!(
                "Failed to fetch bundle {}: {}",
                self.handle,
                String::from_utf8_lossy(&output.stderr)
            );
            return Err(e);
        }

        Ok(Bytes::from(output.stdout))
    }
}

pub enum BundleHandle<'a> {
    External(ExternalHandle<'a>),
    Blob(RawBundle2Id),
}

impl BundleHandle<'static> {
    pub fn blob(id: RawBundle2Id) -> Self {
        Self::Blob(id)
    }
}

impl<'a> BundleHandle<'a> {
    pub async fn load(&self, ctx: &CoreContext, repo: &BlobRepo) -> Result<Bytes, Error> {
        match self {
            Self::External(ref external) => {
                info!(
                    ctx.logger(),
                    "Fetching external bundle: {}", external.handle
                );
                external.load().await
            }
            Self::Blob(ref id) => {
                info!(ctx.logger(), "Fetching raw bundle: {}", id);
                let bytes = id.load(ctx.clone(), repo.blobstore()).compat().await?;
                Ok(bytes.into_bytes())
            }
        }
    }
}

#[derive(Debug)]
pub enum OntoRev {
    Hg(HgChangesetId),
    Bonsai(ChangesetId),
}

pub struct PushrebaseSpec {
    pub onto: BookmarkName,
    pub onto_rev: Option<OntoRev>,
    pub target: Target,
    pub timestamps: HashMap<HgChangesetId, Timestamp>,
    pub recorded_duration: Option<Duration>,
}

pub struct ReplaySpec<'a> {
    pub bundle: BundleHandle<'a>,
    pub pushrebase_spec: PushrebaseSpec,
}

impl ReplaySpec<'static> {
    pub fn from_bookmark_update_log_entry(entry: BookmarkUpdateLogEntry) -> Result<Self, Error> {
        let replay_data = entry
            .reason
            .into_bundle_replay_data()
            .ok_or_else(|| format_err!("Entry has replay data"))?;

        let bundle = BundleHandle::blob(
            Blake2::from_str(&replay_data.bundle_handle).map(RawBundle2Id::new)?,
        );

        let target = entry
            .to_changeset_id
            .ok_or_else(|| format_err!("Replaying bookmark deletions is not supported"))?;

        Ok(ReplaySpec {
            bundle,
            pushrebase_spec: PushrebaseSpec {
                timestamps: replay_data.commit_timestamps,
                onto: entry.bookmark_name,
                onto_rev: entry.from_changeset_id.map(OntoRev::Bonsai),
                target: Target::bonsai(target),
                recorded_duration: None,
            },
        })
    }
}

impl<'a> ReplaySpec<'a> {
    pub fn from_hg_recording_entry<'b>(
        bundle_helper: &'a str,
        entry: HgRecordingEntry,
    ) -> Result<ReplaySpec<'a>, Error> {
        let HgRecordingEntry {
            id,
            onto,
            onto_rev,
            bundle,
            timestamps,
            revs,
            duration,
        } = entry;

        let target = Target::hg(
            *revs
                .last()
                .ok_or_else(|| format_err!("Missing target in HgRecordingEntry {}", id))?,
        );

        let bundle = BundleHandle::External(ExternalHandle {
            bundle_helper,
            handle: bundle,
        });

        Ok(ReplaySpec {
            bundle,
            pushrebase_spec: PushrebaseSpec {
                onto,
                onto_rev: Some(OntoRev::Hg(onto_rev)),
                target,
                timestamps,
                recorded_duration: duration,
            },
        })
    }
}

impl OntoRev {
    pub async fn into_cs_id(
        self,
        ctx: &CoreContext,
        repo: &BlobRepo,
    ) -> Result<ChangesetId, Error> {
        match self {
            Self::Hg(hg_cs_id) => {
                let cs_id = repo
                    .get_bonsai_from_hg(ctx.clone(), hg_cs_id)
                    .compat()
                    .await?
                    .ok_or_else(|| format_err!("Bonsai changeset missing for {:?}", hg_cs_id))?;

                Ok(cs_id)
            }
            Self::Bonsai(cs_id) => Ok(cs_id),
        }
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_load_external_success() -> Result<(), Error> {
        let bundle = ExternalHandle {
            bundle_helper: "printf",
            handle: "foo".to_string(),
        };

        assert_eq!(bundle.load().await?, "foo".as_bytes());
        Ok(())
    }

    #[tokio::test]
    async fn test_load_external_err() -> Result<(), Error> {
        let bundle = ExternalHandle {
            bundle_helper: "false",
            handle: "foo".to_string(),
        };

        assert!(bundle.load().await.is_err());
        Ok(())
    }
}
