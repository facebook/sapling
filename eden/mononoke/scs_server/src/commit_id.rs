/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::collections::{BTreeMap, BTreeSet};

use faster_hex::hex_string;
use futures_util::{future, FutureExt};
use mononoke_api::{ChangesetContext, ChangesetId, MononokeError, RepoContext};
use source_control as thrift;

/// Generate a mapping for a commit's identity into the requested identity
/// schemes.
pub(crate) async fn map_commit_identity(
    changeset_ctx: &ChangesetContext,
    schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>, MononokeError> {
    let mut ids = BTreeMap::new();
    ids.insert(
        thrift::CommitIdentityScheme::BONSAI,
        thrift::CommitId::bonsai(changeset_ctx.id().as_ref().into()),
    );
    let mut scheme_identities = vec![];
    if schemes.contains(&thrift::CommitIdentityScheme::HG) {
        let identity = async {
            if let Some(hg_id) = changeset_ctx.hg_id().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    thrift::CommitIdentityScheme::HG,
                    thrift::CommitId::hg(hg_id.as_ref().into()),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    if schemes.contains(&thrift::CommitIdentityScheme::GLOBALREV) {
        let identity = async {
            if let Some(globalrev) = changeset_ctx.globalrev().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    thrift::CommitIdentityScheme::GLOBALREV,
                    thrift::CommitId::globalrev(globalrev.id() as i64),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    let scheme_identities = future::try_join_all(scheme_identities).await?;
    for maybe_identity in scheme_identities {
        if let Some((scheme, id)) = maybe_identity {
            ids.insert(scheme, id);
        }
    }
    Ok(ids)
}

/// Generate mappings for multiple commits' identities into the requested
/// identity schemes.
pub(crate) async fn map_commit_identities(
    repo_ctx: &RepoContext,
    ids: Vec<ChangesetId>,
    schemes: &BTreeSet<thrift::CommitIdentityScheme>,
) -> Result<
    BTreeMap<ChangesetId, BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
    MononokeError,
> {
    let mut result = BTreeMap::new();
    for id in ids.iter() {
        let mut idmap = BTreeMap::new();
        idmap.insert(
            thrift::CommitIdentityScheme::BONSAI,
            thrift::CommitId::bonsai(id.as_ref().into()),
        );
        result.insert(*id, idmap);
    }
    let mut scheme_identities = vec![];
    if schemes.contains(&thrift::CommitIdentityScheme::HG) {
        let ids = ids.clone();
        let identities = async {
            let bonsai_hg_ids = repo_ctx
                .changeset_hg_ids(ids)
                .await?
                .into_iter()
                .map(|(cs_id, hg_cs_id)| {
                    (
                        cs_id,
                        thrift::CommitIdentityScheme::HG,
                        thrift::CommitId::hg(hg_cs_id.as_ref().into()),
                    )
                })
                .collect::<Vec<_>>();
            let result: Result<_, MononokeError> = Ok(bonsai_hg_ids);
            result
        };
        scheme_identities.push(identities.boxed());
    }
    if schemes.contains(&thrift::CommitIdentityScheme::GLOBALREV) {
        let identities = async {
            let bonsai_globalrev_ids = repo_ctx
                .changeset_globalrev_ids(ids)
                .await?
                .into_iter()
                .map(|(cs_id, globalrev)| {
                    (
                        cs_id,
                        thrift::CommitIdentityScheme::GLOBALREV,
                        thrift::CommitId::globalrev(globalrev.id() as i64),
                    )
                })
                .collect::<Vec<_>>();
            let result: Result<_, MononokeError> = Ok(bonsai_globalrev_ids);
            result
        };
        scheme_identities.push(identities.boxed());
    }
    let scheme_identities = future::try_join_all(scheme_identities).await?;
    for ids in scheme_identities {
        for (cs_id, commit_identity_scheme, commit_id) in ids {
            result
                .entry(cs_id)
                .or_insert_with(BTreeMap::new)
                .insert(commit_identity_scheme, commit_id);
        }
    }
    Ok(result)
}

/// Trait to extend CommitId with useful functions.
pub(crate) trait CommitIdExt {
    fn scheme(&self) -> thrift::CommitIdentityScheme;
    fn to_string(&self) -> String;
}

impl CommitIdExt for thrift::CommitId {
    /// Returns the commit identity scheme of a commit ID.
    fn scheme(&self) -> thrift::CommitIdentityScheme {
        match self {
            thrift::CommitId::bonsai(_) => thrift::CommitIdentityScheme::BONSAI,
            thrift::CommitId::hg(_) => thrift::CommitIdentityScheme::HG,
            thrift::CommitId::git(_) => thrift::CommitIdentityScheme::GIT,
            thrift::CommitId::globalrev(_) => thrift::CommitIdentityScheme::GLOBALREV,
            thrift::CommitId::UnknownField(t) => (*t).into(),
        }
    }

    /// Convert a `thrift::CommitId` to a string for display. This would normally
    /// be implemented as `Display for thrift::CommitId`, but it is defined in
    /// the generated crate.
    fn to_string(&self) -> String {
        match self {
            thrift::CommitId::bonsai(id) => hex_string(&id).expect("hex_string should never fail"),
            thrift::CommitId::hg(id) => hex_string(&id).expect("hex_string should never fail"),
            thrift::CommitId::git(id) => hex_string(&id).expect("hex_string should never fail"),
            thrift::CommitId::globalrev(rev) => rev.to_string(),
            thrift::CommitId::UnknownField(t) => format!("unknown id type ({})", t),
        }
    }
}
