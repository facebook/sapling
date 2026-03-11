/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::borrow::Cow;
use std::collections::BTreeMap;
use std::collections::BTreeSet;

use futures_util::FutureExt;
use futures_util::future;
use maplit::btreeset;
use metaconfig_types::CommitIdentityScheme;
use mononoke_api::ChangesetContext;
use mononoke_api::MononokeError;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use source_control as source_control_thrift;

/// If identity schemes were not provided, get the repo's default identity scheme
/// and use it. This matches the SCS behavior.
fn fall_back_to_default_identity_scheme<'a>(
    repo_ctx: &RepoContext<Repo>,
    schemes: &'a BTreeSet<source_control_thrift::CommitIdentityScheme>,
) -> Result<Cow<'a, BTreeSet<source_control_thrift::CommitIdentityScheme>>, MononokeError> {
    if !schemes.is_empty() {
        return Ok(Cow::Borrowed(schemes));
    }

    let use_default_id_scheme = justknobs::eval(
        "scm/mononoke:use_repo_default_id_scheme_in_scs",
        None,
        Some(repo_ctx.name()),
    )?;

    if !use_default_id_scheme {
        return Ok(Cow::Borrowed(schemes));
    }

    let default_scheme = repo_ctx.config().default_commit_identity_scheme.clone();

    let maybe_translated_scheme = match default_scheme {
        CommitIdentityScheme::HG => Some(source_control_thrift::CommitIdentityScheme::HG),
        CommitIdentityScheme::GIT => Some(source_control_thrift::CommitIdentityScheme::GIT),
        CommitIdentityScheme::BONSAI => Some(source_control_thrift::CommitIdentityScheme::BONSAI),
        _ => None,
    };
    match maybe_translated_scheme {
        Some(translated_scheme) => Ok(Cow::Owned(btreeset! {translated_scheme})),
        None => Ok(Cow::Borrowed(schemes)),
    }
}

/// Generate a mapping for a commit's identity into the requested identity
/// schemes. Uses concurrent resolution via try_join_all, matching SCS behavior.
pub async fn map_commit_identity(
    changeset_ctx: &ChangesetContext<Repo>,
    schemes: &BTreeSet<source_control_thrift::CommitIdentityScheme>,
) -> Result<
    BTreeMap<source_control_thrift::CommitIdentityScheme, source_control_thrift::CommitId>,
    MononokeError,
> {
    let mut ids = BTreeMap::new();
    ids.insert(
        source_control_thrift::CommitIdentityScheme::BONSAI,
        source_control_thrift::CommitId::bonsai(changeset_ctx.id().as_ref().into()),
    );
    let schemes = fall_back_to_default_identity_scheme(changeset_ctx.repo_ctx(), schemes)?;

    let mut scheme_identities = vec![];
    if schemes.contains(&source_control_thrift::CommitIdentityScheme::HG) {
        let identity = async {
            if let Some(hg_id) = changeset_ctx.hg_id().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    source_control_thrift::CommitIdentityScheme::HG,
                    source_control_thrift::CommitId::hg(hg_id.as_ref().into()),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    if schemes.contains(&source_control_thrift::CommitIdentityScheme::GLOBALREV) {
        let identity = async {
            if let Some(globalrev) = changeset_ctx.globalrev().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    source_control_thrift::CommitIdentityScheme::GLOBALREV,
                    source_control_thrift::CommitId::globalrev(globalrev.id() as i64),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    if schemes.contains(&source_control_thrift::CommitIdentityScheme::SVNREV) {
        let identity = async {
            if let Some(svnrev) = changeset_ctx.svnrev().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    source_control_thrift::CommitIdentityScheme::SVNREV,
                    source_control_thrift::CommitId::svnrev(svnrev.id() as i64),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    if schemes.contains(&source_control_thrift::CommitIdentityScheme::GIT) {
        let identity = async {
            if let Some(git_sha1) = changeset_ctx.git_sha1().await? {
                let result: Result<Option<_>, MononokeError> = Ok(Some((
                    source_control_thrift::CommitIdentityScheme::GIT,
                    source_control_thrift::CommitId::git(git_sha1.as_ref().into()),
                )));
                result
            } else {
                Ok(None)
            }
        };
        scheme_identities.push(identity.boxed());
    }
    let scheme_identities = future::try_join_all(scheme_identities).await?;
    for (scheme, id) in scheme_identities.into_iter().flatten() {
        ids.insert(scheme, id);
    }
    Ok(ids)
}
