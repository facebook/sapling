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

use anyhow::format_err;
use anyhow::Error;
use clap::Arg;
use clap::ArgGroup;
use clap::ArgMatches;
use clap::Args;
use faster_hex::hex_decode;
use faster_hex::hex_string;
use futures::future::try_join_all;
use futures::future::FutureExt;
use futures::stream::FuturesOrdered;
use futures::stream::TryStreamExt;
use source_control::types as thrift;

use crate::connection::Connection;

pub(crate) const ARG_COMMIT_ID: &str = "commit-id";
pub(crate) const ARG_BOOKMARK: &str = "bookmark";
pub(crate) const ARG_HG_COMMIT_ID: &str = "hg-commit-id";
pub(crate) const ARG_BONSAI_ID: &str = "bonsai-id";
pub(crate) const ARG_GIT_SHA1: &str = "git-sha1";
pub(crate) const ARG_GLOBALREV: &str = "globalrev";
pub(crate) const ARG_SVNREV: &str = "svnrev";
pub(crate) const ARG_BUBBLE_ID: &str = "bubble-id";
pub(crate) const ARG_SNAPSHOT_ID: &str = "snapshot-id";

#[derive(
    strum_macros::EnumString,
    strum_macros::Display,
    Clone,
    PartialEq,
    Eq,
    Hash
)]
#[strum(serialize_all = "kebab-case")]
pub(crate) enum Scheme {
    Bonsai,
    Hg,
    Git,
    Globalrev,
    Svnrev,
}

impl Scheme {
    pub(crate) fn into_thrift(self) -> thrift::CommitIdentityScheme {
        match self {
            Self::Hg => thrift::CommitIdentityScheme::HG,
            Self::Bonsai => thrift::CommitIdentityScheme::BONSAI,
            Self::Git => thrift::CommitIdentityScheme::GIT,
            Self::Globalrev => thrift::CommitIdentityScheme::GLOBALREV,
            Self::Svnrev => thrift::CommitIdentityScheme::SVNREV,
        }
    }
}

#[derive(Args, Clone)]
pub(crate) struct SchemeArgs {
    #[clap(long, short('S'), default_value = "hg", value_delimiter = ',')]
    /// Commit identity schemes to display
    schemes: Vec<Scheme>,
}

impl SchemeArgs {
    pub(crate) fn into_request_schemes(self) -> BTreeSet<thrift::CommitIdentityScheme> {
        self.schemes.into_iter().map(Scheme::into_thrift).collect()
    }

    pub(crate) fn scheme_string_set(&self) -> HashSet<String> {
        self.schemes.iter().map(ToString::to_string).collect()
    }
}

/// Add arguments for specifying commit_ids.
///
/// The user can specify commit_ids by bookmark, hg commit ID, bonsai changeset ID, or
/// a generic "commit_id", for which we will try to work out which kind of identifier
/// the user has provided.
fn add_commit_id_args_impl(
    cmd: clap::Command<'_>,
    required: bool,
    multiple: bool,
) -> clap::Command<'_> {
    cmd.arg(
        Arg::with_name(ARG_COMMIT_ID)
            .short('i')
            .long("commit-id")
            .takes_value(true)
            .multiple(multiple)
            .number_of_values(1)
            .help("Commit ID to query (bonsai or Hg)"),
    )
    .arg(
        Arg::with_name(ARG_BOOKMARK)
            .short('B')
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
        ArgGroup::with_name("commit")
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

fn get_commit_ids(matches: &ArgMatches) -> Result<Vec<CommitId>, clap::Error> {
    get_commit_ids_impl(matches)
        .map_err(|err| clap::Error::raw(clap::error::ErrorKind::ValueValidation, err))
}

/// Get the commit ids specified by the user.  They are returned in the order specified.
fn get_commit_ids_impl(matches: &ArgMatches) -> Result<Vec<CommitId>, Error> {
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

// Unfortunately we can't use clap derive API here directly because we care about
// the order of arguments, but we still implement proper clap traits so that it
// can be used in conjunction with derive API in other parts of the code.
#[derive(Clone)]
pub(crate) struct CommitIdArgs {
    commit_id: CommitId,
}

impl CommitIdArgs {
    pub fn into_commit_id(self) -> CommitId {
        self.commit_id
    }
}

impl clap::FromArgMatches for CommitIdArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let mut commit_ids = get_commit_ids(matches)?;
        if commit_ids.len() == 1 {
            Ok(CommitIdArgs {
                commit_id: commit_ids.pop().unwrap(),
            })
        } else {
            panic!("expected single commit id")
        }
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        let mut commit_ids = get_commit_ids(matches)?;
        use std::cmp::Ordering;
        match commit_ids.len().cmp(&1) {
            Ordering::Equal => {
                self.commit_id = commit_ids.pop().unwrap();
            }
            Ordering::Less => {}
            Ordering::Greater => panic!("expected single commit id"),
        }

        Ok(())
    }
}

impl clap::Args for CommitIdArgs {
    fn augment_args(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, true, false)
    }

    fn augment_args_for_update(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, true, false)
    }
}

#[derive(Clone)]
pub(crate) struct OptionalCommitIdArgs {
    commit_id: Option<CommitId>,
}

impl OptionalCommitIdArgs {
    pub fn into_commit_id(self) -> Option<CommitId> {
        self.commit_id
    }
}

impl clap::FromArgMatches for OptionalCommitIdArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let commit_ids = get_commit_ids(matches)?;
        Ok(Self {
            commit_id: commit_ids.into_iter().next(),
        })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        let commit_ids = get_commit_ids(matches)?;
        self.commit_id = commit_ids.into_iter().next();
        Ok(())
    }
}

impl clap::Args for OptionalCommitIdArgs {
    fn augment_args(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, false, false)
    }

    fn augment_args_for_update(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, false, false)
    }
}

#[derive(Clone)]
/// 0 or more commit ids
pub(crate) struct CommitIdsArgs {
    commit_ids: Vec<CommitId>,
}

impl CommitIdsArgs {
    pub fn into_commit_ids(self) -> Vec<CommitId> {
        self.commit_ids
    }
}

impl clap::FromArgMatches for CommitIdsArgs {
    fn from_arg_matches(matches: &ArgMatches) -> Result<Self, clap::Error> {
        let commit_ids = get_commit_ids(matches)?;
        Ok(CommitIdsArgs { commit_ids })
    }

    fn update_from_arg_matches(&mut self, matches: &ArgMatches) -> Result<(), clap::Error> {
        self.commit_ids = get_commit_ids(matches)?;
        Ok(())
    }
}

impl clap::Args for CommitIdsArgs {
    fn augment_args(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, false, true)
    }

    fn augment_args_for_update(cmd: clap::Command<'_>) -> clap::Command<'_> {
        add_commit_id_args_impl(cmd, false, true)
    }
}

/// A `CommitId` is any of the ways a user can specify a commit.
#[derive(Clone)]
pub(crate) enum CommitId {
    /// Commit ID is of an unknown type that must be resolved.
    Resolve(String),

    /// Bonsai ID.
    BonsaiId([u8; 32]),

    /// Bonsai ID with bubble
    EphemeralBonsai([u8; 32], Option<NonZeroU64>),

    /// Hg commit ID.
    HgId([u8; 20]),

    /// Git SHA-1.
    GitSha1([u8; 20]),

    /// Globalrev.
    Globalrev(u64),

    /// SVN revision.
    Svnrev(u64),

    /// A bookmark name.
    Bookmark(String),
}

impl CommitId {
    pub(crate) fn into_bookmark_name(self) -> Result<String, Error> {
        match self {
            Self::Bookmark(name) => Ok(name),
            _ => anyhow::bail!("expected bookmark name (got {})", self),
        }
    }
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
