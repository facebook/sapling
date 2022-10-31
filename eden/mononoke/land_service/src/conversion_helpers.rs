/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;

use anyhow::anyhow;
use anyhow::Result;
use blobstore::Loadable;
use bookmarks_movement::BookmarkKindRestrictions;
use cloned::cloned;
use futures::future::TryFutureExt;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use hooks::CrossRepoPushSource;
use land_service_if::types::*;
use mononoke_api::CoreContext;
use mononoke_api::MononokeError;
use mononoke_api::RepoContext;
use mononoke_types::private::Bytes;
use mononoke_types::BonsaiChangeset;
use mononoke_types::ChangesetId;
use pushrebase::PushrebaseChangesetPair;

use crate::errors;
use crate::errors::LandChangesetsError;

/// Convert BTreeSet of ChangetSetIds to a Hashset of BonsaiChangeset
pub async fn convert_bonsai_changesets(
    changesets: BTreeSet<Vec<u8>>,
    ctx: &CoreContext,
    repo: &RepoContext,
) -> Result<HashSet<BonsaiChangeset>, LandChangesetsError> {
    let blobstore = repo.blob_repo().blobstore();
    let changeset_ids = changesets
        .into_iter()
        .map(convert_changeset_id_from_bytes)
        .collect::<Result<HashSet<_>, LandChangesetsError>>()?;

    let changesets: HashSet<BonsaiChangeset> = stream::iter(changeset_ids)
        .map(|cs_id| {
            cloned!(ctx);
            async move {
                cs_id
                    .load(&ctx, blobstore)
                    .map_err(MononokeError::from)
                    .await
            }
        })
        .buffer_unordered(100)
        .try_collect()
        .await?;
    Ok(changesets)
}

pub fn convert_changeset_id_from_bytes(
    bonsai: Vec<u8>,
) -> Result<ChangesetId, LandChangesetsError> {
    Ok(ChangesetId::from_bytes(bonsai)?)
}

/// Convert a pushvars map from thrift's representation to the one used
/// internally in mononoke.
pub(crate) fn convert_pushvars(pushvars: BTreeMap<String, Vec<u8>>) -> HashMap<String, Bytes> {
    pushvars
        .into_iter()
        .map(|(name, value)| (name, Bytes::from(value)))
        .collect()
}

pub(crate) fn convert_hex_to_str(changeset: &[u8]) -> String {
    faster_hex::hex_string(changeset)
}

/// Convert bookmark restrictions from the bookmark in the request
pub fn convert_bookmark_restrictions(
    bookmark_restrictions: land_service_if::BookmarkKindRestrictions,
) -> Result<BookmarkKindRestrictions, LandChangesetsError> {
    match bookmark_restrictions {
        land_service_if::BookmarkKindRestrictions::ANY_KIND => {
            Ok(BookmarkKindRestrictions::AnyKind)
        }
        land_service_if::BookmarkKindRestrictions::ONLY_SCRATCH => {
            Ok(BookmarkKindRestrictions::OnlyScratch)
        }
        land_service_if::BookmarkKindRestrictions::ONLY_PUBLISHING => {
            Ok(BookmarkKindRestrictions::OnlyPublishing)
        }
        other => Err(LandChangesetsError::InternalError(errors::internal_error(
            anyhow!("Unknown BookmarkKindRestrictions: {}", other).as_ref(),
        ))),
    }
}

/// Convert cross repo push source from the cross_repo_push_source in the request
pub fn convert_cross_repo_push_source(
    cross_repo_push_source: land_service_if::CrossRepoPushSource,
) -> Result<CrossRepoPushSource, LandChangesetsError> {
    match cross_repo_push_source {
        land_service_if::CrossRepoPushSource::NATIVE_TO_THIS_REPO => {
            Ok(CrossRepoPushSource::NativeToThisRepo)
        }
        land_service_if::CrossRepoPushSource::PUSH_REDIRECTED => {
            Ok(CrossRepoPushSource::PushRedirected)
        }
        other => Err(LandChangesetsError::InternalError(errors::internal_error(
            anyhow!("Unknown CrossRepoPushSource: {}", other).as_ref(),
        ))),
    }
}

/// Convert vec of PushrebaseChangesetPair and converts it to a vec of BonsaiHashPairs
pub fn convert_rebased_changesets_into_pairs(
    rebased_changeset: PushrebaseChangesetPair,
) -> BonsaiHashPairs {
    BonsaiHashPairs {
        old_id: rebased_changeset.id_old.as_ref().to_vec(),
        new_id: rebased_changeset.id_new.as_ref().to_vec(),
    }
}

/// Convert usize and to i64
pub fn convert_to_i64(val: usize) -> Result<i64, LandChangesetsError> {
    val.try_into()
        .map_err(|_| anyhow!("usize too big for i64").into())
}

/// Converts option of ChangesetId to vec binary used in thrift to represent ChangesetId
pub fn convert_changeset_id_to_vec_binary(
    old_bookmark_value: ChangesetId,
) -> land_service_if::ChangesetId {
    old_bookmark_value.as_ref().to_vec()
}
