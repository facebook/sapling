/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

//! Arguments for Commit IDs and Commit Identity Schemes

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::fmt;
use std::num::NonZeroU64;

use anyhow::bail;
use anyhow::format_err;
use anyhow::Error;
use clap::App;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use faster_hex::hex_decode;
use faster_hex::hex_string;
use futures_util::future::try_join_all;
use futures_util::future::FutureExt;
use futures_util::stream::FuturesOrdered;
use futures_util::stream::TryStreamExt;
use source_control::types as thrift;

use crate::connection::Connection;

pub(crate) const ARG_SCHEMES: &str = "SCHEMES";
pub(crate) const ARG_COMMIT_ID: &str = "COMMIT_ID";
pub(crate) const ARG_BOOKMARK: &str = "BOOKMARK";
pub(crate) const ARG_HG_COMMIT_ID: &str = "HG_COMMIT_ID";
pub(crate) const ARG_BONSAI_ID: &str = "BONSAI_ID";
pub(crate) const ARG_GIT_SHA1: &str = "GIT_SHA1";
pub(crate) const ARG_GLOBALREV: &str = "GLOBALREV";
pub(crate) const ARG_SVNREV: &str = "SVNREV";
pub(crate) const ARG_BUBBLE_ID: &str = "BUBBLE_ID";
pub(crate) const ARG_SNAPSHOT_ID: &str = "SNAPSHOT_ID";

pub(crate) const ARG_GROUP_COMMIT_ID: &str = "GROUP_COMMIT_ID";

/// Add arguments to specify a set of commit identity schemes.
pub(crate) fn add_scheme_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_SCHEMES)
            .short("S")
            .long("schemes")
            .takes_value(true)
            .possible_values(&["bonsai", "hg", "git", "globalrev", "svnrev"])
            .multiple(true)
            .use_delimiter(true)
            .number_of_values(1)
            .help("Commit identity schemes to display")
            .default_value("hg"),
    )
}

/// Get the schemes specified as a set of scheme names.
pub(crate) fn get_schemes(matches: &ArgMatches) -> HashSet<String> {
    matches
        .values_of(ARG_SCHEMES)
        .expect("schemes required")
        .map(|s| s.to_owned())
        .collect()
}

/// Get the schemes specified as a set of thrift schemes.
pub(crate) fn get_request_schemes(matches: &ArgMatches) -> BTreeSet<thrift::CommitIdentityScheme> {
    matches
        .values_of(ARG_SCHEMES)
        .expect("schemes required")
        .filter_map(|scheme| match scheme {
            "hg" => Some(thrift::CommitIdentityScheme::HG),
            "bonsai" => Some(thrift::CommitIdentityScheme::BONSAI),
            "git" => Some(thrift::CommitIdentityScheme::GIT),
            "globalrev" => Some(thrift::CommitIdentityScheme::GLOBALREV),
            "svnrev" => Some(thrift::CommitIdentityScheme::SVNREV),
            _ => None,
        })
        .collect()
}

/// Map commit IDs to the scheme name and string representation of the commit ID.
pub(crate) fn map_commit_ids<'a>(
    ids: impl Iterator<Item = &'a thrift::CommitId>,
) -> BTreeMap<String, String> {
    ids.filter_map(map_commit_id).collect()
}

/// Map a commit ID to its scheme name and string representation.
pub(crate) fn map_commit_id(id: &thrift::CommitId) -> Option<(String, String)> {
    match id {
        thrift::CommitId::bonsai(hash) => Some((String::from("bonsai"), hex_string(hash))),
        thrift::CommitId::hg(hash) => Some((String::from("hg"), hex_string(hash))),
        thrift::CommitId::git(hash) => Some((String::from("git"), hex_string(hash))),
        thrift::CommitId::globalrev(rev) => Some((String::from("globalrev"), rev.to_string())),
        thrift::CommitId::svnrev(rev) => Some((String::from("svnrev"), rev.to_string())),
        _ => None,
    }
}

/// Add arguments for specifying commit_ids.
///
/// The user can specify commit_ids by bookmark, hg commit ID, bonsai changeset ID, or
/// a generic "commit_id", for which we will ry to work out which kind of identifier
/// the user has provided.
fn add_commit_id_args_impl<'a, 'b>(
    app: App<'a, 'b>,
    required: bool,
    multiple: bool,
) -> App<'a, 'b> {
    app.arg(
        Arg::with_name(ARG_COMMIT_ID)
            .short("i")
            .long("commit-id")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Commit ID to query (bonsai or Hg)"),
    )
    .arg(
        Arg::with_name(ARG_BOOKMARK)
            .short("B")
            .long("bookmark")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Bookmark to query"),
    )
    .arg(
        Arg::with_name(ARG_HG_COMMIT_ID)
            .long("hg-commit-id")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Hg commit ID to query"),
    )
    .arg(
        Arg::with_name(ARG_BONSAI_ID)
            .long("bonsai-id")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Bonsai ID to query"),
    )
    .arg(
        Arg::with_name(ARG_SNAPSHOT_ID)
            .long("snapshot-id")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Snapshot ID to query"),
    )
    .arg(
        Arg::with_name(ARG_GIT_SHA1)
            .long("git")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Git SHA-1 to query"),
    )
    .arg(
        Arg::with_name(ARG_GLOBALREV)
            .long("globalrev")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Globalrev to query"),
    )
    .arg(
        Arg::with_name(ARG_SVNREV)
            .long("svnrev")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("SVN revision to query"),
    )
    .arg(
        Arg::with_name(ARG_BUBBLE_ID)
            .long("bubble-id")
            .takes_value(true)
            .multiple(false)
            .number_of_values(1)
            .help("Bubble id on which to check the bonsai commits"),
    )
    .group(
        ArgGroup::with_name(ARG_GROUP_COMMIT_ID)
            .args(&[
                ARG_COMMIT_ID,
                ARG_BOOKMARK,
                ARG_HG_COMMIT_ID,
                ARG_BONSAI_ID,
                ARG_SNAPSHOT_ID,
                ARG_GIT_SHA1,
                ARG_GLOBALREV,
                ARG_SVNREV,
            ])
            .multiple(multiple)
            .required(required),
    )
}

/// Add arguments for specifying a single commit ID
pub(crate) fn add_commit_id_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_commit_id_args_impl(app, true, false)
}

/// Add arguments for specifying an optional single commit ID
pub(crate) fn add_optional_commit_id_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_commit_id_args_impl(app, false, false)
}

/// Add arguments for specifying multiple commit IDs (at least one)
pub(crate) fn add_multiple_commit_id_args<'a, 'b>(app: App<'a, 'b>) -> App<'a, 'b> {
    add_commit_id_args_impl(app, true, true)
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

/// Get the commit ids specified by the user.  They are returned in the order specified.
pub(crate) fn get_commit_ids(matches: &ArgMatches<'_>) -> Result<Vec<CommitId>, Error> {
    let bubble_id = matches
        .value_of(ARG_BUBBLE_ID)
        .map(|id| id.parse::<NonZeroU64>())
        .transpose()?;
    let mut commit_ids = BTreeMap::new();
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_BOOKMARK),
        matches.values_of(ARG_BOOKMARK),
    ) {
        for (index, value) in indices.zip(values) {
            commit_ids.insert(index, CommitId::Bookmark(value.to_string()));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_HG_COMMIT_ID),
        matches.values_of(ARG_HG_COMMIT_ID),
    ) {
        for (index, value) in indices.zip(values) {
            let mut id = [0; 20];
            hex_decode(value.as_bytes(), &mut id)?;
            commit_ids.insert(index, CommitId::HgId(id));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_BONSAI_ID),
        matches.values_of(ARG_BONSAI_ID),
    ) {
        for (index, value) in indices.zip(values) {
            let mut id = [0; 32];
            hex_decode(value.as_bytes(), &mut id)?;
            commit_ids.insert(index, CommitId::BonsaiId(id));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_SNAPSHOT_ID),
        matches.values_of(ARG_SNAPSHOT_ID),
    ) {
        for (index, value) in indices.zip(values) {
            let mut id = [0; 32];
            hex_decode(value.as_bytes(), &mut id)?;
            commit_ids.insert(index, CommitId::EphemeralBonsai(id, bubble_id));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_GIT_SHA1),
        matches.values_of(ARG_GIT_SHA1),
    ) {
        for (index, value) in indices.zip(values) {
            let mut hash = [0; 20];
            hex_decode(value.as_bytes(), &mut hash)?;
            commit_ids.insert(index, CommitId::GitSha1(hash));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_GLOBALREV),
        matches.values_of(ARG_GLOBALREV),
    ) {
        for (index, value) in indices.zip(values) {
            commit_ids.insert(index, CommitId::Globalrev(value.parse::<u64>()?));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_SVNREV),
        matches.values_of(ARG_SVNREV),
    ) {
        for (index, value) in indices.zip(values) {
            commit_ids.insert(index, CommitId::Svnrev(value.parse::<u64>()?));
        }
    }
    if let (Some(indices), Some(values)) = (
        matches.indices_of(ARG_COMMIT_ID),
        matches.values_of(ARG_COMMIT_ID),
    ) {
        for (index, value) in indices.zip(values) {
            commit_ids.insert(index, CommitId::Resolve(value.to_string()));
        }
    }
    Ok(commit_ids
        .into_iter()
        .map(|(_index, commit_id)| commit_id)
        .collect())
}

/// Get a single commit ID specified by the user.
pub(crate) fn get_commit_id(matches: &ArgMatches<'_>) -> Result<CommitId, Error> {
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() != 1 {
        bail!("expected 1 commit_id (got {})", commit_ids.len())
    }
    Ok(commit_ids.into_iter().next().expect("commit id expected"))
}

/// Gets a single bookmark name specified by the user
pub(crate) fn get_bookmark_name(matches: &ArgMatches<'_>) -> Result<String, Error> {
    let commit_ids = get_commit_ids(matches)?;
    if commit_ids.len() != 1 {
        bail!("expected 1 bookmark name (got multiple commits ids)",)
    }
    let commit_id = commit_ids.into_iter().next().expect("commit id expected");
    let bookmark_name = match commit_id {
        CommitId::Bookmark(bookmark_name) => bookmark_name,
        _ => bail!("expected bookmark name (got {})", commit_id,),
    };
    Ok(bookmark_name)
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
