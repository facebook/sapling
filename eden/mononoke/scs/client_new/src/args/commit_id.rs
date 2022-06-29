/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for Commit IDs and Commit Identity Schemes

use std::fmt;
use std::num::NonZeroU64;

use anyhow::format_err;
use anyhow::Error;
use clap::ArgGroup;
use clap::Args;
use faster_hex::hex_decode;
use faster_hex::hex_string;
use futures_util::future::try_join_all;
use futures_util::future::FutureExt;
use futures_util::stream::FuturesOrdered;
use futures_util::stream::TryStreamExt;
use source_control::types as thrift;

use crate::connection::Connection;

#[derive(Args, Clone)]
#[clap(group(
    ArgGroup::new("commit")
    .required(true)
    .args(&["commit-id", "bookmark", "hg-commit-id", "bonsai-id",
        "snapshot-id", "git", "globalrev", "svnrev"]),
))]
pub(crate) struct CommitIdArgs {
    #[clap(long, short = 'i')]
    /// Commit ID to query (bonsai or Hg)
    commit_id: Option<String>,
    #[clap(long, short = 'B')]
    /// Bookmark to query
    bookmark: Option<String>,
    #[clap(long)]
    /// Hg commit ID to query
    hg_commit_id: Option<String>,
    #[clap(long)]
    /// Bonsai ID to query
    bonsai_id: Option<String>,
    #[clap(long)]
    /// Snapshot ID to query
    snapshot_id: Option<String>,
    #[clap(long)]
    /// Git SHA-1 to query
    git: Option<String>,
    #[clap(long)]
    /// Globalrev to query
    globalrev: Option<u64>,
    #[clap(long)]
    /// SVN revision to query
    svnrev: Option<u64>,

    #[clap(long, requires = "snapshot-id")]
    /// Bubble id on which to check the bonsai commits.
    bubble_id: Option<u64>,
}

impl CommitIdArgs {
    pub fn into_commit_id(self) -> Result<CommitId, Error> {
        let bubble_id = self.bubble_id.and_then(NonZeroU64::new);
        Ok(if let Some(bookmark) = self.bookmark {
            CommitId::Bookmark(bookmark)
        } else if let Some(id_str) = self.hg_commit_id {
            let mut id = [0; 20];
            hex_decode(id_str.as_bytes(), &mut id)?;
            CommitId::HgId(id)
        } else if let Some(id_str) = self.bonsai_id {
            let mut id = [0; 32];
            hex_decode(id_str.as_bytes(), &mut id)?;
            CommitId::BonsaiId(id)
        } else if let Some(id_str) = self.snapshot_id {
            let mut id = [0; 32];
            hex_decode(id_str.as_bytes(), &mut id)?;
            CommitId::EphemeralBonsai(id, bubble_id)
        } else if let Some(id_str) = self.git {
            let mut id = [0; 20];
            hex_decode(id_str.as_bytes(), &mut id)?;
            CommitId::GitSha1(id)
        } else if let Some(id) = self.globalrev {
            CommitId::Globalrev(id)
        } else if let Some(id) = self.svnrev {
            CommitId::Svnrev(id)
        } else if let Some(id) = self.commit_id {
            CommitId::Resolve(id)
        } else {
            anyhow::bail!("Missing commit id")
        })
    }
}

/// A `CommitId` is any of the ways a user can specify a commit.
pub(crate) enum CommitId {
    /// Commit ID is of an unknown type that must be resolved.
    Resolve(String),

    /// Bonsai ID.
    BonsaiId([u8; 32]),

    /// Bonsai ID with bubble
    EphemeralBonsai([u8; 32], Option<NonZeroU64>),

    /// Hg commit ID.
    HgId([u8; 20]),

    // Git SHA-1.
    GitSha1([u8; 20]),

    // Globalrev.
    Globalrev(u64),

    // SVN revision.
    Svnrev(u64),

    /// A bookmark name.
    Bookmark(String),
}

impl fmt::Display for CommitId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            CommitId::Resolve(id) => write!(f, "commit id '{}'", id),
            CommitId::BonsaiId(bonsai) => write!(f, "bonsai id '{}'", hex_string(bonsai)),
            CommitId::EphemeralBonsai(bonsai, bubble_id) => write!(
                f,
                "bonsai id '{}' on bubble {}",
                hex_string(bonsai),
                bubble_id.map_or_else(|| "unknown".to_string(), |id| id.to_string()),
            ),
            CommitId::HgId(id) => write!(f, "hg commit id '{}'", hex_string(id)),
            CommitId::GitSha1(id) => write!(f, "git sha1 '{}'", hex_string(id)),
            CommitId::Globalrev(rev) => write!(f, "globalrev '{}'", rev),
            CommitId::Svnrev(rev) => write!(f, "svn revision '{}'", rev),
            CommitId::Bookmark(bookmark) => write!(f, "bookmark '{}'", bookmark),
        }
    }
}

/// Try to resolve a bookmark name to a commit ID.
async fn try_resolve_bookmark(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    bookmark: impl Into<String>,
) -> Result<Option<thrift::CommitId>, Error> {
    let params = thrift::RepoResolveBookmarkParams {
        bookmark_name: bookmark.into(),
        identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let response = conn.repo_resolve_bookmark(repo, &params).await?;
    Ok(response
        .ids
        .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
}

/// Try to resolve a hex string to an hg commit ID (it can be prefix of the full hash)
async fn try_resolve_hg_commit_id(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    value: impl AsRef<str>,
) -> Result<Option<thrift::CommitId>, Error> {
    // Possible prefix should be valid to be passed to `repo_resolve_commit_prefix`:
    if value.as_ref().len() > 40 || value.as_ref().chars().any(|c| !c.is_digit(16)) {
        return Ok(None);
    }

    let resp = conn
        .repo_resolve_commit_prefix(
            repo,
            &thrift::RepoResolveCommitPrefixParams {
                identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
                    .into_iter()
                    .collect(),
                prefix_scheme: thrift::CommitIdentityScheme::HG,
                prefix: value.as_ref().into(),
                ..Default::default()
            },
        )
        .await?;

    if let thrift::RepoResolveCommitPrefixResponseType::AMBIGUOUS = resp.resolved_type {
        eprintln!(
            "note: several hg commits with the prefix '{}' exist",
            value.as_ref()
        );
    }

    Ok(resp
        .ids
        .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
}

/// Try to resolve a hex string to a bonsai changeset ID (it can be prefix of the full hash)
async fn try_resolve_bonsai_id(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    value: impl AsRef<str>,
) -> Result<Option<thrift::CommitId>, Error> {
    // Possible prefix should be valid to be passed to `repo_resolve_commit_prefix`:
    if value.as_ref().len() > 64 || value.as_ref().chars().any(|c| !c.is_digit(16)) {
        return Ok(None);
    }

    let resp = conn
        .repo_resolve_commit_prefix(
            repo,
            &thrift::RepoResolveCommitPrefixParams {
                identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
                    .into_iter()
                    .collect(),
                prefix_scheme: thrift::CommitIdentityScheme::BONSAI,
                prefix: value.as_ref().into(),
                ..Default::default()
            },
        )
        .await?;

    if let thrift::RepoResolveCommitPrefixResponseType::AMBIGUOUS = resp.resolved_type {
        eprintln!(
            "note: several bonsai commits with the prefix '{}' exist",
            value.as_ref()
        );
    }

    Ok(resp
        .ids
        .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
}

/// Try to resolve a git sha1 string to a commit ID.
async fn try_resolve_git_sha1(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    value: impl AsRef<str>,
) -> Result<Option<thrift::CommitId>, Error> {
    let mut id = [0; 20];
    if hex_decode(value.as_ref().as_bytes(), &mut id).is_err() {
        return Ok(None);
    }
    let params = thrift::CommitLookupParams {
        identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
            .into_iter()
            .collect(),
        ..Default::default()
    };
    let commit = thrift::CommitSpecifier {
        repo: repo.clone(),
        id: thrift::CommitId::git(id.to_vec()),
        ..Default::default()
    };
    let response = conn.commit_lookup(&commit, &params).await?;
    Ok(response
        .ids
        .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
}

/// Try to resolve a globalrev string to a commit ID.
async fn try_resolve_globalrev(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    globalrev: impl AsRef<str>,
) -> Result<Option<thrift::CommitId>, Error> {
    match globalrev.as_ref().parse::<i64>() {
        Ok(globalrev) => {
            let params = thrift::CommitLookupParams {
                identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
                    .into_iter()
                    .collect(),
                ..Default::default()
            };
            let commit = thrift::CommitSpecifier {
                repo: repo.clone(),
                id: thrift::CommitId::globalrev(globalrev),
                ..Default::default()
            };
            let response = conn.commit_lookup(&commit, &params).await?;
            Ok(response
                .ids
                .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
        }
        Err(_) => Ok(None),
    }
}

/// Try to resolve a svn revision number to a commit ID.
async fn try_resolve_svnrev(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    globalrev: impl AsRef<str>,
) -> Result<Option<thrift::CommitId>, Error> {
    match globalrev.as_ref().parse::<i64>() {
        Ok(globalrev) => {
            let params = thrift::CommitLookupParams {
                identity_schemes: Some(thrift::CommitIdentityScheme::BONSAI)
                    .into_iter()
                    .collect(),
                ..Default::default()
            };
            let commit = thrift::CommitSpecifier {
                repo: repo.clone(),
                id: thrift::CommitId::svnrev(globalrev),
                ..Default::default()
            };
            let response = conn.commit_lookup(&commit, &params).await?;
            Ok(response
                .ids
                .and_then(|ids| ids.get(&thrift::CommitIdentityScheme::BONSAI).cloned()))
        }
        Err(_) => Ok(None),
    }
}

/// Resolve commit IDs into a thrift commit identifier.
///
/// Simple commit ID types (hashes of a known type) are converted to the right
/// commit ID variant.
///
/// Other commit ID types, like bookmark names or hashes of an unknown type may
/// involve a call to the server to resolve.
pub(crate) async fn resolve_commit_ids(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    commit_ids: impl IntoIterator<Item = &CommitId>,
) -> Result<Vec<thrift::CommitId>, Error> {
    commit_ids
        .into_iter()
        .map(|commit_id| {
            async move {
                match commit_id {
                    CommitId::BonsaiId(bonsai) => Ok(thrift::CommitId::bonsai(bonsai.to_vec())),
                    CommitId::EphemeralBonsai(bonsai, bubble) => Ok(
                        thrift::CommitId::ephemeral_bonsai(thrift::EphemeralBonsai {
                            bonsai_id: bonsai.to_vec(),
                            bubble_id: bubble.map_or(0, NonZeroU64::get).try_into()?,
                            ..Default::default()
                        }),
                    ),
                    CommitId::HgId(hg) => Ok(thrift::CommitId::hg(hg.to_vec())),
                    CommitId::GitSha1(hash) => Ok(thrift::CommitId::git(hash.to_vec())),
                    CommitId::Globalrev(rev) => Ok(thrift::CommitId::globalrev((*rev).try_into()?)),
                    CommitId::Svnrev(rev) => Ok(thrift::CommitId::svnrev((*rev).try_into()?)),
                    CommitId::Bookmark(bookmark) => try_resolve_bookmark(conn, repo, bookmark)
                        .await?
                        .ok_or_else(|| format_err!("bookmark not found: {}", bookmark)),
                    CommitId::Resolve(commit_id) => {
                        let resolvers = vec![
                            try_resolve_bonsai_id(conn, repo, commit_id).boxed(),
                            try_resolve_hg_commit_id(conn, repo, commit_id).boxed(),
                            try_resolve_git_sha1(conn, repo, commit_id).boxed(),
                            try_resolve_globalrev(conn, repo, commit_id).boxed(),
                            try_resolve_svnrev(conn, repo, commit_id).boxed(),
                        ];
                        let candidates: Vec<_> = try_join_all(resolvers.into_iter())
                            .await?
                            .into_iter()
                            .flatten()
                            .collect();
                        match candidates.as_slice() {
                            [] => Err(format_err!("commit not found: {}", commit_id)),
                            [id] => Ok(id.clone()),
                            _ => {
                                // This commit ID resolves to different
                                // commits in different schemes. This is a
                                // cross-scheme hash collision (e.g. two
                                // commits exist where the Hg commit ID of
                                // one matches the Git commit ID of the
                                // other).
                                //
                                // In practice this should be very rare, but
                                // handle it here. Users will need to
                                // specify which scheme should be used by
                                // using the appropriate argument to specify
                                // the ID (e.g. --hg-commit-id).
                                Err(format_err!("ambiguous commit id: {}", commit_id))
                            }
                        }
                    }
                }
            }
        })
        .collect::<FuturesOrdered<_>>()
        .try_collect()
        .await
}

/// Resolve a single commit ID.
pub(crate) async fn resolve_commit_id(
    conn: &Connection,
    repo: &thrift::RepoSpecifier,
    commit_id: &CommitId,
) -> Result<thrift::CommitId, Error> {
    let commit_ids = resolve_commit_ids(conn, repo, Some(commit_id).into_iter()).await?;
    Ok(commit_ids.into_iter().next().expect("commit id expected"))
}
