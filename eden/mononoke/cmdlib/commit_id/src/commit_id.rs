/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![feature(trait_alias)]

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use anyhow::anyhow;
use bonsai_git_mapping::BonsaiGitMappingRef;
use bonsai_globalrev_mapping::BonsaiGlobalrevMappingRef;
use bonsai_hg_mapping::BonsaiHgMappingRef;
use bonsai_svnrev_mapping::BonsaiSvnrevMappingRef;
use bookmarks::BookmarkKey;
use bookmarks::BookmarksRef;
use clap::ValueEnum;
pub use commit_id_types::*;
use context::CoreContext;
use futures::TryStreamExt;
use futures::future::join;
use futures::stream::FuturesOrdered;
use mercurial_types::HgChangesetId;
use mononoke_types::ChangesetId;
use mononoke_types::Globalrev;
use mononoke_types::Svnrev;
use mononoke_types::hash::GitSha1;
use strum::Display;

pub trait Repo =
    BonsaiHgMappingRef + BonsaiGitMappingRef + BonsaiGlobalrevMappingRef + BonsaiSvnrevMappingRef;

#[derive(Copy, Clone, Eq, PartialEq, ValueEnum, Display)]
#[strum(serialize_all = "kebab_case")]
pub enum IdentityScheme {
    /// Mononoke bonsai hash
    Bonsai,

    /// Mercurial hash
    Hg,

    /// Git SHA-1 hash
    Git,

    /// Globalrev
    Globalrev,

    /// Subversion revision (legacy)
    Svnrev,
}

impl IdentityScheme {
    /// Parse a commit id of this scheme from a string.
    pub async fn parse_commit_id(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
        commit_id: &str,
    ) -> Result<ChangesetId> {
        let cs_id = match self {
            IdentityScheme::Bonsai => commit_id
                .parse::<ChangesetId>()
                .context("Invalid bonsai changeset id")?,
            IdentityScheme::Hg => {
                let hg_cs_id = commit_id
                    .parse::<HgChangesetId>()
                    .context("Invalid hg changeset id")?;
                repo.bonsai_hg_mapping()
                    .get_bonsai_from_hg(ctx, hg_cs_id)
                    .await?
                    .ok_or_else(|| anyhow!("hg-bonsai mapping not found for {}", hg_cs_id))?
            }
            IdentityScheme::Git => {
                let git_id = commit_id
                    .parse::<GitSha1>()
                    .context("Invalid git changeset id")?;
                repo.bonsai_git_mapping()
                    .get_bonsai_from_git_sha1(ctx, git_id)
                    .await?
                    .ok_or_else(|| anyhow!("git-bonsai mapping not found for {}", git_id))?
            }
            IdentityScheme::Globalrev => {
                let globalrev = commit_id
                    .parse::<Globalrev>()
                    .context("Invalid globalrev")?;
                repo.bonsai_globalrev_mapping()
                    .get_bonsai_from_globalrev(ctx, globalrev)
                    .await?
                    .ok_or_else(|| {
                        anyhow!("globalrev-bonsai mapping not found for {}", globalrev)
                    })?
            }
            IdentityScheme::Svnrev => {
                let svnrev = commit_id.parse::<Svnrev>().context("Invalid svnrev")?;
                repo.bonsai_svnrev_mapping()
                    .get_bonsai_from_svnrev(ctx, svnrev)
                    .await?
                    .ok_or_else(|| anyhow!("svnrev-bonsai mapping not found for {}", svnrev))?
            }
        };
        Ok(cs_id)
    }

    /// Map a commit id into a string for this identity scheme.
    ///
    /// Returns `None` if this commit does not exist in that scheme.
    pub async fn map_commit_id(
        &self,
        ctx: &CoreContext,
        repo: &impl Repo,
        cs_id: ChangesetId,
    ) -> Result<Option<String>> {
        let commit_id = match self {
            IdentityScheme::Bonsai => Some(cs_id.to_string()),
            IdentityScheme::Hg => repo
                .bonsai_hg_mapping()
                .get_hg_from_bonsai(ctx, cs_id)
                .await?
                .as_ref()
                .map(ToString::to_string),
            IdentityScheme::Git => repo
                .bonsai_git_mapping()
                .get_git_sha1_from_bonsai(ctx, cs_id)
                .await?
                .as_ref()
                .map(ToString::to_string),
            IdentityScheme::Globalrev => repo
                .bonsai_globalrev_mapping()
                .get_globalrev_from_bonsai(ctx, cs_id)
                .await?
                .as_ref()
                .map(ToString::to_string),
            IdentityScheme::Svnrev => repo
                .bonsai_svnrev_mapping()
                .get_svnrev_from_bonsai(ctx, cs_id)
                .await?
                .as_ref()
                .map(ToString::to_string),
        };
        Ok(commit_id)
    }
}

/// Parse a general commit ID from a string
///
/// The string can either be of the form <scheme>=<id>, or just
/// a bare id, in which case the scheme will be inferred.
///
/// For inferred schemes, globalrevs should be prefixed by 'm', and svnrevs
/// should be prefixed by 's'.
///
/// Hash types are inferred from their length (64 characters for
/// 32-byte bonsai hashes, 40 characters for 20-byte Mercurial or
/// Git hashes).  For Mercurial and Git, whichever one exists is
/// selected.  In the unlikely event that the hash refers to both
/// a Mercurial commit and a Git commit, the Mercurial commit is
/// returned.
pub async fn parse_commit_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    commit_id: &str,
) -> Result<ChangesetId> {
    if let Some((scheme, id)) = commit_id.split_once('=') {
        let scheme = IdentityScheme::from_str(scheme, /* ignore_case */ true).map_err(|e| {
            anyhow!(
                "Failed to parse commit identity scheme '{}': {}",
                scheme.to_string(),
                e
            )
        })?;
        scheme.parse_commit_id(ctx, repo, id).await
    } else if let Some(globalrev) = commit_id.strip_prefix('m') {
        IdentityScheme::Globalrev
            .parse_commit_id(ctx, repo, globalrev)
            .await
    } else if let Some(svnrev) = commit_id.strip_prefix('s') {
        IdentityScheme::Svnrev
            .parse_commit_id(ctx, repo, svnrev)
            .await
    } else if commit_id.len() == 64 {
        IdentityScheme::Bonsai
            .parse_commit_id(ctx, repo, commit_id)
            .await
    } else if commit_id.len() == 40 {
        match join(
            IdentityScheme::Hg.parse_commit_id(ctx, repo, commit_id),
            IdentityScheme::Git.parse_commit_id(ctx, repo, commit_id),
        )
        .await
        {
            (Ok(cs_id), _) => Ok(cs_id),
            (Err(_), Ok(cs_id)) => Ok(cs_id),
            (Err(e), Err(_)) => Err(e),
        }
    } else {
        Err(anyhow!("Invalid commit id: {}", commit_id))
    }
}

/// Print a commit id in the selected schemes.
///
/// If a single scheme is requested, just the commit id is printed, and
/// an error is returned if the commit does not exist in that scheme.
///
/// Otherwise, each commit id is prefixed by the name of the scheme, and
/// schemes for which the commit does not exist are omitted.
///
/// If no schemes are selected, prints the bonsai hash.
pub async fn print_commit_id(
    ctx: &CoreContext,
    repo: &impl Repo,
    schemes: &[IdentityScheme],
    cs_id: ChangesetId,
) -> Result<()> {
    match schemes {
        [] => {
            println!("{}", cs_id);
        }
        [scheme] => {
            let commit_id = scheme
                .map_commit_id(ctx, repo, cs_id)
                .await?
                .ok_or_else(|| {
                    anyhow!(
                        "bonsai-{} mapping not found for {}",
                        scheme.to_string(),
                        cs_id
                    )
                })?;
            println!("{}", commit_id);
        }
        schemes => {
            for scheme in schemes {
                if let Some(commit_id) = scheme.map_commit_id(ctx, repo, cs_id).await? {
                    println!("{}: {}", scheme, commit_id);
                }
            }
        }
    }
    Ok(())
}

/// Resolve commit hashes or bookmarks into ChangesetIds.
pub async fn resolve_commit_ids(
    ctx: &CoreContext,
    repo: &(impl Repo + BookmarksRef),
    commit_ids: impl IntoIterator<Item = &CommitId>,
) -> Result<Vec<ChangesetId>, Error> {
    commit_ids
        .into_iter()
        .map(|commit_id| async move {
            match commit_id {
                CommitId::BonsaiId(bonsai) => {
                    IdentityScheme::Bonsai
                        .parse_commit_id(ctx, repo, std::str::from_utf8(bonsai)?)
                        .await
                }
                CommitId::HgId(hg) => {
                    IdentityScheme::Hg
                        .parse_commit_id(ctx, repo, std::str::from_utf8(hg)?)
                        .await
                }
                CommitId::GitSha1(hash) => {
                    IdentityScheme::Git
                        .parse_commit_id(ctx, repo, std::str::from_utf8(hash)?)
                        .await
                }
                CommitId::Globalrev(rev) => {
                    IdentityScheme::Globalrev
                        .parse_commit_id(ctx, repo, &rev.to_string())
                        .await
                }
                CommitId::Svnrev(rev) => {
                    IdentityScheme::Svnrev
                        .parse_commit_id(ctx, repo, &rev.to_string())
                        .await
                }
                CommitId::Bookmark(bookmark) => repo
                    .bookmarks()
                    .get(
                        ctx.clone(),
                        &BookmarkKey::new(bookmark)?,
                        bookmarks::Freshness::MostRecent,
                    )
                    .await
                    .and_then(|cs_id| {
                        cs_id.ok_or_else(|| anyhow!("bookmark {} not found", bookmark))
                    }),
                CommitId::Resolve(cs_id_hash) => parse_commit_id(ctx, repo, cs_id_hash).await,
                _ => Err(anyhow!("Unsupported commit id type"))?,
            }
        })
        .collect::<FuturesOrdered<_>>()
        .try_collect()
        .await
}

/// Resolve a single commit ID.
pub async fn resolve_commit_id(
    ctx: &CoreContext,
    repo: &(impl Repo + BookmarksRef),
    commit_id: &CommitId,
) -> Result<ChangesetId> {
    let commit_ids = resolve_commit_ids(ctx, repo, Some(commit_id).into_iter()).await?;
    Ok(commit_ids.into_iter().next().expect("commit id expected"))
}

pub async fn resolve_optional_commit_id(
    ctx: &CoreContext,
    repo: &(impl Repo + BookmarksRef),
    commit_id: Option<&CommitId>,
) -> Result<Option<ChangesetId>> {
    if let Some(commit_id) = commit_id {
        Ok(Some(resolve_commit_id(ctx, repo, commit_id).await?))
    } else {
        Ok(None)
    }
}
