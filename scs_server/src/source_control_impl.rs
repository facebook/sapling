/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use std::cmp::min;
use std::collections::{BTreeMap, BTreeSet};
use std::convert::{TryFrom, TryInto};
use std::fmt::{Debug, Display};
use std::ops::RangeBounds;
use std::sync::Arc;

use async_trait::async_trait;
use bytes::{BufMut, Bytes};
use context::generate_session_id;
use faster_hex::hex_string;
use fbinit::FacebookInit;
use futures::stream::Stream;
use futures_preview::{
    compat::Future01CompatExt, compat::Stream01CompatExt, stream, StreamExt, TryStreamExt,
};
use futures_util::{future::FutureExt, try_future::try_join_all, try_join};
use mercurial_types::Globalrev;
use mononoke_api::{
    ChangesetContext, ChangesetId, ChangesetPathContext, ChangesetPathDiffContext,
    ChangesetSpecifier, CoreContext, FileContext, FileId, FileMetadata, FileType, HgChangesetId,
    Mononoke, MononokeError, PathEntry, RepoContext, SessionContainer, TreeContext, TreeEntry,
    TreeId,
};
use mononoke_types::hash::{Sha1, Sha256};
use scuba_ext::ScubaSampleBuilder;
use slog::Logger;
use source_control as thrift;
use source_control::server::SourceControlService;
use source_control::services::source_control_service as service;
use sshrelay::SshEnvVars;
use tracing::TraceContext;
use xdiff;

const MAX_LIMIT: i64 = 1000;
const MAX_CHUNK_SIZE: i64 = 16 * 1024 * 1024;
// Magic number used when we want to limit concurrency with buffer_unordered.
const CONCURRENCY_LIMIT: usize = 100;

trait SpecifierExt {
    fn description(&self) -> String;

    fn scuba_reponame(&self) -> Option<String> {
        None
    }

    fn scuba_commit(&self) -> Option<String> {
        None
    }

    fn scuba_path(&self) -> Option<String> {
        None
    }
}

impl SpecifierExt for thrift::RepoSpecifier {
    fn description(&self) -> String {
        format!("repo={}", self.name)
    }

    fn scuba_reponame(&self) -> Option<String> {
        Some(self.name.clone())
    }
}

impl SpecifierExt for thrift::CommitSpecifier {
    fn description(&self) -> String {
        format!("repo={} commit={}", self.repo.name, self.id.to_string())
    }

    fn scuba_reponame(&self) -> Option<String> {
        self.repo.scuba_reponame()
    }

    fn scuba_commit(&self) -> Option<String> {
        Some(self.id.to_string())
    }
}

impl SpecifierExt for thrift::CommitPathSpecifier {
    fn description(&self) -> String {
        format!(
            "repo={} commit={} path={}",
            self.commit.repo.name,
            self.commit.id.to_string(),
            self.path
        )
    }

    fn scuba_reponame(&self) -> Option<String> {
        self.commit.scuba_reponame()
    }
    fn scuba_commit(&self) -> Option<String> {
        self.commit.scuba_commit()
    }
    fn scuba_path(&self) -> Option<String> {
        Some(self.path.clone())
    }
}

impl SpecifierExt for thrift::TreeSpecifier {
    fn description(&self) -> String {
        match self {
            thrift::TreeSpecifier::by_commit_path(commit_path) => commit_path.description(),
            thrift::TreeSpecifier::by_id(tree_id) => format!(
                "repo={} tree={}",
                tree_id.repo.name,
                hex_string(&tree_id.id).expect("hex_string should never fail")
            ),
            thrift::TreeSpecifier::UnknownField(n) => format!("unknown tree specifier type {}", n),
        }
    }

    fn scuba_reponame(&self) -> Option<String> {
        match self {
            thrift::TreeSpecifier::by_commit_path(commit_path) => commit_path.scuba_reponame(),
            thrift::TreeSpecifier::by_id(tree_id) => tree_id.repo.scuba_reponame(),
            thrift::TreeSpecifier::UnknownField(_) => None,
        }
    }

    fn scuba_commit(&self) -> Option<String> {
        match self {
            thrift::TreeSpecifier::by_commit_path(commit_path) => commit_path.scuba_commit(),
            thrift::TreeSpecifier::by_id(_tree_id) => None,
            thrift::TreeSpecifier::UnknownField(_) => None,
        }
    }

    fn scuba_path(&self) -> Option<String> {
        match self {
            thrift::TreeSpecifier::by_commit_path(commit_path) => commit_path.scuba_path(),
            thrift::TreeSpecifier::by_id(_tree_id) => None,
            thrift::TreeSpecifier::UnknownField(_) => None,
        }
    }
}

impl SpecifierExt for thrift::FileSpecifier {
    fn description(&self) -> String {
        match self {
            thrift::FileSpecifier::by_commit_path(commit_path) => commit_path.description(),
            thrift::FileSpecifier::by_id(file_id) => format!(
                "repo={} file={}",
                file_id.repo.name,
                hex_string(&file_id.id).expect("hex_string should never fail"),
            ),
            thrift::FileSpecifier::by_sha1_content_hash(hash) => format!(
                "repo={} file_sha1={}",
                hash.repo.name,
                hex_string(&hash.content_hash).expect("hex_string should never fail"),
            ),
            thrift::FileSpecifier::by_sha256_content_hash(hash) => format!(
                "repo={} file_sha256={}",
                hash.repo.name,
                hex_string(&hash.content_hash).expect("hex_string should never fail"),
            ),
            thrift::FileSpecifier::UnknownField(n) => format!("unknown file specifier type {}", n),
        }
    }

    fn scuba_reponame(&self) -> Option<String> {
        match self {
            thrift::FileSpecifier::by_commit_path(commit_path) => commit_path.scuba_reponame(),
            thrift::FileSpecifier::by_id(file_id) => file_id.repo.scuba_reponame(),
            thrift::FileSpecifier::by_sha1_content_hash(hash) => hash.repo.scuba_reponame(),
            thrift::FileSpecifier::by_sha256_content_hash(hash) => hash.repo.scuba_reponame(),
            thrift::FileSpecifier::UnknownField(_) => None,
        }
    }
    fn scuba_commit(&self) -> Option<String> {
        match self {
            thrift::FileSpecifier::by_commit_path(commit_path) => commit_path.scuba_commit(),
            thrift::FileSpecifier::by_id(_file_id) => None,
            thrift::FileSpecifier::by_sha1_content_hash(_hash) => None,
            thrift::FileSpecifier::by_sha256_content_hash(_hash) => None,
            thrift::FileSpecifier::UnknownField(_) => None,
        }
    }
    fn scuba_path(&self) -> Option<String> {
        match self {
            thrift::FileSpecifier::by_commit_path(commit_path) => commit_path.scuba_path(),
            thrift::FileSpecifier::by_id(_file_id) => None,
            thrift::FileSpecifier::by_sha1_content_hash(_hash) => None,
            thrift::FileSpecifier::by_sha256_content_hash(_hash) => None,
            thrift::FileSpecifier::UnknownField(_) => None,
        }
    }
}

#[derive(Clone)]
pub struct SourceControlServiceImpl {
    fb: FacebookInit,
    mononoke: Arc<Mononoke>,
    logger: Logger,
    scuba_builder: ScubaSampleBuilder,
}

impl SourceControlServiceImpl {
    pub fn new(
        fb: FacebookInit,
        mononoke: Arc<Mononoke>,
        logger: Logger,
        scuba_builder: ScubaSampleBuilder,
    ) -> Self {
        Self {
            fb,
            mononoke,
            logger,
            scuba_builder,
        }
    }

    fn create_ctx(&self, specifier: Option<&dyn SpecifierExt>) -> CoreContext {
        let mut scuba = self.scuba_builder.clone();
        scuba.add_common_server_data().add("type", "thrift");
        if let Some(specifier) = specifier {
            if let Some(reponame) = specifier.scuba_reponame() {
                scuba.add("reponame", reponame);
            }
            if let Some(commit) = specifier.scuba_commit() {
                scuba.add("commit", commit);
            }
            if let Some(path) = specifier.scuba_path() {
                scuba.add("path", path);
            }
        }
        let session_id = generate_session_id();
        scuba.add("session_uuid", session_id.to_string());

        let session = SessionContainer::new(
            self.fb,
            session_id,
            TraceContext::default(),
            None,
            None,
            SshEnvVars::default(),
            None,
        );

        session.new_context(self.logger.clone(), scuba)
    }

    /// Get the repo specified by a `thrift::RepoSpecifier`.
    fn repo(
        &self,
        ctx: CoreContext,
        repo: &thrift::RepoSpecifier,
    ) -> Result<RepoContext, errors::ServiceError> {
        let repo = self
            .mononoke
            .repo(ctx, &repo.name)?
            .ok_or_else(|| errors::repo_not_found(repo.description()))?;
        Ok(repo)
    }

    /// Get the repo and changeset specified by a `thrift::CommitSpecifier`.
    async fn repo_changeset(
        &self,
        ctx: CoreContext,
        commit: &thrift::CommitSpecifier,
    ) -> Result<(RepoContext, ChangesetContext), errors::ServiceError> {
        let repo = self.repo(ctx, &commit.repo)?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let changeset = repo
            .changeset(changeset_specifier)
            .await?
            .ok_or_else(|| errors::commit_not_found(commit.description()))?;
        Ok((repo, changeset))
    }

    /// Get the repo and tree specified by a `thrift::TreeSpecifier`.
    ///
    /// Returns `None` if the tree is specified by commit path and that path
    /// is not a directory in that commit.
    async fn repo_tree(
        &self,
        ctx: CoreContext,
        tree: &thrift::TreeSpecifier,
    ) -> Result<(RepoContext, Option<TreeContext>), errors::ServiceError> {
        let (repo, tree) = match tree {
            thrift::TreeSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.tree().await?)
            }
            thrift::TreeSpecifier::by_id(tree_id) => {
                let repo = self.repo(ctx, &tree_id.repo)?;
                let tree_id = TreeId::from_request(&tree_id.id)?;
                let tree = repo
                    .tree(tree_id)
                    .await?
                    .ok_or_else(|| errors::tree_not_found(tree.description()))?;
                (repo, Some(tree))
            }
            thrift::TreeSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "tree specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, tree))
    }

    /// Get the repo and file specified by a `thrift::FileSpecifier`.
    ///
    /// Returns `None` if the file is specified by commit path, and that path
    /// is not a file in that commit.
    async fn repo_file(
        &self,
        ctx: CoreContext,
        file: &thrift::FileSpecifier,
    ) -> Result<(RepoContext, Option<FileContext>), errors::ServiceError> {
        let (repo, file) = match file {
            thrift::FileSpecifier::by_commit_path(commit_path) => {
                let (repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
                let path = changeset.path(&commit_path.path)?;
                (repo, path.file().await?)
            }
            thrift::FileSpecifier::by_id(file_id) => {
                let repo = self.repo(ctx, &file_id.repo)?;
                let file_id = FileId::from_request(&file_id.id)?;
                let file = repo
                    .file(file_id)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha1_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo)?;
                let file_sha1 = Sha1::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha1(file_sha1)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::by_sha256_content_hash(hash) => {
                let repo = self.repo(ctx, &hash.repo)?;
                let file_sha256 = Sha256::from_request(&hash.content_hash)?;
                let file = repo
                    .file_by_content_sha256(file_sha256)
                    .await?
                    .ok_or_else(|| errors::file_not_found(file.description()))?;
                (repo, Some(file))
            }
            thrift::FileSpecifier::UnknownField(id) => {
                return Err(errors::invalid_request(format!(
                    "file specifier type not supported: {}",
                    id
                ))
                .into());
            }
        };
        Ok((repo, file))
    }
}

/// Generate a mapping for a commit's identity into the requested identity
/// schemes.
async fn map_commit_identity(
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
    let scheme_identities = try_join_all(scheme_identities).await?;
    for maybe_identity in scheme_identities {
        if let Some((scheme, id)) = maybe_identity {
            ids.insert(scheme, id);
        }
    }
    Ok(ids)
}

/// Generate mappings for multiple commits' identities into the requested
/// identity schemes.
async fn map_commit_identities(
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
    let scheme_identities = try_join_all(scheme_identities).await?;
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

// Diff file against other file.
async fn changeset_path_diff(
    old_path: &Option<ChangesetPathContext>,
    new_path: &Option<ChangesetPathContext>,
    copy_info: thrift::CopyInfo,
    context_lines: usize,
) -> Result<thrift::Diff, errors::ServiceError> {
    // Helper for getting file information.
    async fn get_file_data(
        path: &Option<ChangesetPathContext>,
    ) -> Result<Option<xdiff::DiffFile<String, Bytes>>, errors::ServiceError> {
        match path {
            Some(path) => {
                if let Some(file_type) = path.file_type().await? {
                    let file = path.file().await?.ok_or_else(|| {
                        errors::internal_error("assertion error: file should exist")
                    })?;
                    let contents = file
                        .content()
                        .await
                        .compat()
                        .try_concat()
                        .await
                        .map_err(errors::internal_error)?;
                    let file_type = match file_type {
                        FileType::Regular => xdiff::FileType::Regular,
                        FileType::Executable => xdiff::FileType::Executable,
                        FileType::Symlink => xdiff::FileType::Symlink,
                    };
                    Ok(Some(xdiff::DiffFile {
                        path: path.to_string(),
                        contents,
                        file_type,
                    }))
                } else {
                    Ok(None)
                }
            }
            None => Ok(None),
        }
    }

    // Helper for checking if we should mark the diff as binary
    fn is_binary(
        old_diff_file: &Option<xdiff::DiffFile<String, Bytes>>,
        new_diff_file: &Option<xdiff::DiffFile<String, Bytes>>,
    ) -> bool {
        old_diff_file
            .as_ref()
            .map(|f| f.contents.contains(&0))
            .unwrap_or(false)
            || new_diff_file
                .as_ref()
                .map(|f| f.contents.contains(&0))
                .unwrap_or(false)
    }

    let (old_diff_file, new_diff_file) =
        try_join!(get_file_data(&old_path), get_file_data(&new_path))?;
    let is_binary = is_binary(&old_diff_file, &new_diff_file);
    let copy_info = match copy_info {
        thrift::CopyInfo::NONE => xdiff::CopyInfo::None,
        thrift::CopyInfo::MOVE => xdiff::CopyInfo::Move,
        thrift::CopyInfo::COPY => xdiff::CopyInfo::Copy,
        // thrift is using numbers under the hood so I have to add a default case
        _ => Err(errors::internal_error("unexpected value of copy_info!"))?,
    };
    let opts = xdiff::DiffOpts {
        context: context_lines,
        copy_info,
    };
    let raw_diff = Some(xdiff::diff_unified(old_diff_file, new_diff_file, opts));
    Ok(thrift::Diff::raw_diff(thrift::RawDiff {
        raw_diff,
        is_binary,
    }))
}

/// Trait to extend CommitId with useful functions.
trait CommitIdExt {
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

trait FromRequest<T> {
    fn from_request(t: &T) -> Result<Self, thrift::RequestError>
    where
        Self: Sized;
}

impl FromRequest<thrift::CommitId> for ChangesetSpecifier {
    fn from_request(commit: &thrift::CommitId) -> Result<Self, thrift::RequestError> {
        match commit {
            thrift::CommitId::bonsai(id) => {
                let cs_id = ChangesetId::from_bytes(&id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e.to_string()
                    ))
                })?;
                Ok(ChangesetSpecifier::Bonsai(cs_id))
            }
            thrift::CommitId::hg(id) => {
                let hg_cs_id = HgChangesetId::from_bytes(&id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e.to_string()
                    ))
                })?;
                Ok(ChangesetSpecifier::Hg(hg_cs_id))
            }
            thrift::CommitId::globalrev(rev) => {
                let rev = Globalrev::new((*rev).try_into().map_err(|_| {
                    errors::invalid_request(format!("cannot parse globalrev {} to u64", rev))
                })?);
                Ok(ChangesetSpecifier::Globalrev(rev))
            }
            _ => Err(errors::invalid_request(format!(
                "unsupported commit identity scheme ({})",
                commit.scheme()
            ))),
        }
    }
}

macro_rules! impl_from_request_binary_id(
    ($t:ty, $name:expr) => {
        impl FromRequest<Vec<u8>> for $t {
            fn from_request(id: &Vec<u8>) -> Result<Self, thrift::RequestError> {
                <$t>::from_bytes(id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid {} ({}): {}",
                        $name,
                        hex_string(&id).expect("hex_string should never fail"),
                        e.to_string(),
                    ))})
            }
        }
    }
);

impl_from_request_binary_id!(TreeId, "tree id");
impl_from_request_binary_id!(FileId, "file id");
impl_from_request_binary_id!(Sha1, "sha-1");
impl_from_request_binary_id!(Sha256, "sha-256");

/// Check that an input value is in range for the request, and convert it to
/// the internal type.  Returns a invalid request error if the number was out
/// of range, and an internal error if the conversion failed.
fn check_range_and_convert<F, T, B>(
    name: &'static str,
    value: F,
    range: B,
) -> Result<T, errors::ServiceError>
where
    F: Copy + Display + PartialOrd,
    T: TryFrom<F>,
    B: Debug + RangeBounds<F>,
    <T as TryFrom<F>>::Error: Display,
{
    if range.contains(&value) {
        T::try_from(value).map_err(|e| {
            let msg = format!("failed to convert {} ({}): {}", name, value, e);
            errors::internal_error(msg).into()
        })
    } else {
        let msg = format!("{} ({}) out of range ({:?})", name, value, range);
        Err(errors::invalid_request(msg).into())
    }
}

trait IntoResponse<T> {
    fn into_response(self) -> T;
}

impl IntoResponse<thrift::EntryType> for FileType {
    fn into_response(self) -> thrift::EntryType {
        match self {
            FileType::Regular => thrift::EntryType::FILE,
            FileType::Executable => thrift::EntryType::EXEC,
            FileType::Symlink => thrift::EntryType::LINK,
        }
    }
}

impl IntoResponse<thrift::TreeEntry> for (String, TreeEntry) {
    fn into_response(self) -> thrift::TreeEntry {
        let (name, entry) = self;
        let (type_, info) = match entry {
            TreeEntry::Directory(dir) => {
                let summary = dir.summary();
                let info = thrift::TreeInfo {
                    id: dir.id().as_ref().to_vec(),
                    simple_format_sha1: summary.simple_format_sha1.as_ref().to_vec(),
                    simple_format_sha256: summary.simple_format_sha256.as_ref().to_vec(),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                };
                (thrift::EntryType::TREE, thrift::EntryInfo::tree(info))
            }
            TreeEntry::File(file) => {
                let info = thrift::FileInfo {
                    id: file.content_id().as_ref().to_vec(),
                    file_size: file.size() as i64,
                    content_sha1: file.content_sha1().as_ref().to_vec(),
                    content_sha256: file.content_sha256().as_ref().to_vec(),
                };
                (
                    file.file_type().into_response(),
                    thrift::EntryInfo::file(info),
                )
            }
        };
        thrift::TreeEntry { name, type_, info }
    }
}

impl IntoResponse<thrift::FileInfo> for FileMetadata {
    fn into_response(self) -> thrift::FileInfo {
        thrift::FileInfo {
            id: self.content_id.as_ref().to_vec(),
            file_size: self.total_size as i64,
            content_sha1: self.sha1.as_ref().to_vec(),
            content_sha256: self.sha256.as_ref().to_vec(),
        }
    }
}

#[async_trait]
trait AsyncIntoResponse<T> {
    async fn into_response(self) -> Result<T, errors::ServiceError>;
}

#[async_trait]
impl AsyncIntoResponse<Option<thrift::FilePathInfo>> for ChangesetPathContext {
    async fn into_response(self) -> Result<Option<thrift::FilePathInfo>, errors::ServiceError> {
        let path = &self;
        let (meta, type_) = try_join!(
            async {
                let file = path.file().await?;
                match file {
                    Some(file) => Ok(Some(file.metadata().await?)),
                    None => Ok(None),
                }
            },
            path.file_type()
        )?;
        if let (Some(meta), Some(type_)) = (meta, type_) {
            Ok(Some(thrift::FilePathInfo {
                path: path.to_string(),
                type_: type_.into_response(),
                info: meta.into_response(),
            }))
        } else {
            Ok(None)
        }
    }
}

#[async_trait]
impl AsyncIntoResponse<thrift::CommitCompareFile> for ChangesetPathDiffContext {
    async fn into_response(self) -> Result<thrift::CommitCompareFile, errors::ServiceError> {
        let (other_file, base_file, copy_info) = match self {
            ChangesetPathDiffContext::Added(base_context) => {
                let entry = base_context.into_response().await?;
                (None, entry, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Removed(other_context) => {
                let entry = other_context.into_response().await?;
                (entry, None, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Changed(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::NONE)
            }
            ChangesetPathDiffContext::Copied(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::COPY)
            }
            ChangesetPathDiffContext::Moved(base_context, other_context) => {
                let (other_entry, base_entry) =
                    try_join!(other_context.into_response(), base_context.into_response(),)?;
                (other_entry, base_entry, thrift::CopyInfo::MOVE)
            }
        };
        Ok(thrift::CommitCompareFile {
            base_file,
            other_file,
            copy_info,
        })
    }
}
mod errors {
    use super::{service, thrift};
    use mononoke_api::MononokeError;

    pub(super) enum ServiceError {
        Request(thrift::RequestError),
        Internal(thrift::InternalError),
        Mononoke(MononokeError),
    }

    impl From<thrift::RequestError> for ServiceError {
        fn from(e: thrift::RequestError) -> Self {
            Self::Request(e)
        }
    }

    impl From<thrift::InternalError> for ServiceError {
        fn from(e: thrift::InternalError) -> Self {
            Self::Internal(e)
        }
    }

    impl From<MononokeError> for ServiceError {
        fn from(e: MononokeError) -> Self {
            Self::Mononoke(e)
        }
    }

    macro_rules! impl_into_thrift_error(
        ($t:ty) => {
            impl From<ServiceError> for $t {
                fn from(e: ServiceError) -> Self {
                    match e {
                        ServiceError::Request(e) => e.into(),
                        ServiceError::Internal(e) => e.into(),
                        ServiceError::Mononoke(e) => e.into(),
                    }
                }
            }
        }
    );

    impl_into_thrift_error!(service::RepoResolveBookmarkExn);
    impl_into_thrift_error!(service::RepoListBookmarksExn);
    impl_into_thrift_error!(service::CommitFileDiffsExn);
    impl_into_thrift_error!(service::CommitLookupExn);
    impl_into_thrift_error!(service::CommitInfoExn);
    impl_into_thrift_error!(service::CommitCompareExn);
    impl_into_thrift_error!(service::CommitIsAncestorOfExn);
    impl_into_thrift_error!(service::CommitPathInfoExn);
    impl_into_thrift_error!(service::TreeListExn);
    impl_into_thrift_error!(service::FileExistsExn);
    impl_into_thrift_error!(service::FileInfoExn);
    impl_into_thrift_error!(service::FileContentChunkExn);
    impl_into_thrift_error!(service::CommitLookupXrepoExn);

    pub(super) fn invalid_request(reason: impl ToString) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::INVALID_REQUEST,
            reason: reason.to_string(),
        }
    }

    pub(super) fn internal_error(error: impl ToString) -> thrift::InternalError {
        thrift::InternalError {
            reason: error.to_string(),
            backtrace: None,
        }
    }

    pub(super) fn repo_not_found(repo: String) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::REPO_NOT_FOUND,
            reason: format!("repo not found ({})", repo),
        }
    }

    pub(super) fn commit_not_found(commit: String) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::COMMIT_NOT_FOUND,
            reason: format!("commit not found ({})", commit),
        }
    }

    pub(super) fn file_not_found(file: String) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::FILE_NOT_FOUND,
            reason: format!("file not found ({})", file),
        }
    }

    pub(super) fn tree_not_found(tree: String) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::TREE_NOT_FOUND,
            reason: format!("tree not found ({})", tree),
        }
    }

    pub(super) fn diff_input_too_big(total_size: u64) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::INVALID_REQUEST_INPUT_TOO_BIG,
            reason: format!(
                "only {} bytes of files (in total) can be diffed in one request, you asked for {} bytes",
                thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT, total_size,
            ),
        }
    }

    pub(super) fn diff_input_too_many_paths(path_count: usize) -> thrift::RequestError {
        thrift::RequestError {
            kind: thrift::RequestErrorKind::INVALID_REQUEST_TOO_MANY_PATHS,
            reason: format!(
                "only at most {} paths can be diffed in one request, you asked for {}",
                thrift::consts::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT,
                path_count,
            ),
        }
    }
}

#[async_trait]
impl SourceControlService for SourceControlServiceImpl {
    async fn list_repos(
        &self,
        _params: thrift::ListReposParams,
    ) -> Result<Vec<thrift::Repo>, service::ListReposExn> {
        let _ctx = self.create_ctx(None);
        let mut repo_names: Vec<_> = self
            .mononoke
            .repo_names()
            .map(|repo_name| repo_name.to_string())
            .collect();
        repo_names.sort();
        let rsp = repo_names
            .into_iter()
            .map(|repo_name| thrift::Repo { name: repo_name })
            .collect();
        Ok(rsp)
    }

    /// Resolve a bookmark to a changeset.
    ///
    /// Returns whether the bookmark exists, and the IDs of the changeset in
    /// the requested indentity schemes.
    async fn repo_resolve_bookmark(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoResolveBookmarkParams,
    ) -> Result<thrift::RepoResolveBookmarkResponse, service::RepoResolveBookmarkExn> {
        let ctx = self.create_ctx(Some(&repo));
        let repo = self.repo(ctx, &repo)?;
        match repo.resolve_bookmark(params.bookmark_name).await? {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::RepoResolveBookmarkResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::RepoResolveBookmarkResponse {
                exists: false,
                ids: None,
            }),
        }
    }

    /// List bookmarks.
    async fn repo_list_bookmarks(
        &self,
        repo: thrift::RepoSpecifier,
        params: thrift::RepoListBookmarksParams,
    ) -> Result<thrift::RepoListBookmarksResponse, service::RepoListBookmarksExn> {
        let ctx = self.create_ctx(Some(&repo));
        let limit = match check_range_and_convert("limit", params.limit, 0..=MAX_LIMIT)? {
            0 => None,
            limit => Some(limit),
        };
        let prefix = if !params.bookmark_prefix.is_empty() {
            Some(params.bookmark_prefix)
        } else {
            None
        };
        let repo = self.repo(ctx, &repo)?;
        let bookmarks = repo
            .list_bookmarks(params.include_scratch, prefix, limit)
            .collect()
            .compat()
            .await?;
        let ids = bookmarks.iter().map(|(_name, cs_id)| *cs_id).collect();
        let id_mapping = map_commit_identities(&repo, ids, &params.identity_schemes).await?;
        let bookmarks = bookmarks
            .into_iter()
            .map(|(name, cs_id)| match id_mapping.get(&cs_id) {
                Some(ids) => (name, ids.clone()),
                None => (name, BTreeMap::new()),
            })
            .collect();
        Ok(thrift::RepoListBookmarksResponse { bookmarks })
    }

    /// Look up commit.
    async fn commit_lookup(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self.repo(ctx, &commit.repo)?;
        match repo
            .changeset(ChangesetSpecifier::from_request(&commit.id)?)
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
            }),
        }
    }

    /// Get diff.
    async fn commit_file_diffs(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitFileDiffsParams,
    ) -> Result<thrift::CommitFileDiffsResponse, service::CommitFileDiffsExn> {
        let ctx = self.create_ctx(Some(&commit));
        let context_lines = params.context as usize;

        // Check the path count limit
        if params.paths.len() as i64 > thrift::consts::COMMIT_FILE_DIFFS_PATH_COUNT_LIMIT {
            Err(errors::diff_input_too_many_paths(params.paths.len()))?;
        }

        // Resolve the CommitSpecfier into ChangesetContext
        let other_commit = thrift::CommitSpecifier {
            repo: commit.repo.clone(),
            id: params.other_commit_id.clone(),
        };
        let ((_repo1, base_commit), (_repo2, other_commit)) = try_join!(
            self.repo_changeset(ctx.clone(), &commit),
            self.repo_changeset(ctx.clone(), &other_commit,)
        )?;

        // Resolve the path into ChangesetPathContext
        let paths = params
            .paths
            .into_iter()
            .map(|path_pair| {
                Ok((
                    match path_pair.base_path {
                        Some(path) => Some(base_commit.path(path)?),
                        None => None,
                    },
                    match path_pair.other_path {
                        Some(path) => Some(other_commit.path(path)?),
                        None => None,
                    },
                    path_pair.copy_info,
                ))
            })
            .collect::<Result<Vec<_>, errors::ServiceError>>()?;

        // Check the total file size limit
        let flat_paths = paths
            .iter()
            .flat_map(|(base_path, other_path, _)| vec![base_path, other_path])
            .filter_map(|x| x.as_ref());
        let total_input_size: u64 = try_join_all(flat_paths.map(|path| {
            async move {
                let r: Result<_, errors::ServiceError> = if let Some(file) = path.file().await? {
                    Ok(file.metadata().await?.total_size)
                } else {
                    Ok(0)
                };
                r
            }
        }))
        .await?
        .into_iter()
        .sum();

        if total_input_size as i64 > thrift::consts::COMMIT_FILE_DIFFS_SIZE_LIMIT {
            Err(errors::diff_input_too_big(total_input_size))?;
        }

        let path_diffs =
            try_join_all(paths.into_iter().map(|(base_path, other_path, copy_info)| {
                async move {
                    let diff =
                        changeset_path_diff(&other_path, &base_path, copy_info, context_lines)
                            .await?;
                    let r: Result<_, errors::ServiceError> =
                        Ok(thrift::CommitFileDiffsResponseElement {
                            base_path: base_path.map(|p| p.to_string()),
                            other_path: other_path.map(|p| p.to_string()),
                            diff,
                        });
                    r
                }
            }))
            .await?;
        Ok(thrift::CommitFileDiffsResponse { path_diffs })
    }

    /// Get commit info.
    async fn commit_info(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitInfoParams,
    ) -> Result<thrift::CommitInfo, service::CommitInfoExn> {
        let ctx = self.create_ctx(Some(&commit));
        let (repo, changeset) = self.repo_changeset(ctx, &commit).await?;

        async fn map_parent_identities(
            repo: &RepoContext,
            changeset: &ChangesetContext,
            identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
        ) -> Result<Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>, MononokeError>
        {
            let parents = changeset.parents().await?;
            let parent_id_mapping =
                map_commit_identities(&repo, parents.clone(), identity_schemes).await?;
            Ok(parents
                .iter()
                .map(|parent_id| {
                    parent_id_mapping
                        .get(parent_id)
                        .map(Clone::clone)
                        .unwrap_or_else(BTreeMap::new)
                })
                .collect())
        }

        let (ids, message, date, author, parents, extra) = try_join!(
            map_commit_identity(&changeset, &params.identity_schemes),
            changeset.message(),
            changeset.author_date(),
            changeset.author(),
            map_parent_identities(&repo, &changeset, &params.identity_schemes),
            changeset.extras(),
        )?;
        Ok(thrift::CommitInfo {
            ids,
            message,
            date: date.timestamp(),
            tz: date.offset().local_minus_utc(),
            author,
            parents,
            extra: extra.into_iter().collect(),
        })
    }

    /// Returns `true` if this commit is an ancestor of `other_commit`.
    async fn commit_is_ancestor_of(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitIsAncestorOfParams,
    ) -> Result<bool, service::CommitIsAncestorOfExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self.repo(ctx, &commit.repo)?;
        let changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(&params.other_commit_id)?;
        let (changeset, other_changeset_id) = try_join!(
            repo.changeset(changeset_specifier),
            repo.resolve_specifier(other_changeset_specifier),
        )?;
        let changeset = changeset.ok_or_else(|| errors::commit_not_found(commit.description()))?;
        let other_changeset_id = other_changeset_id.ok_or_else(|| {
            errors::commit_not_found(format!(
                "repo={} commit={}",
                commit.repo.name,
                params.other_commit_id.to_string()
            ))
        })?;
        let is_ancestor_of = changeset.is_ancestor_of(other_changeset_id).await?;
        Ok(is_ancestor_of)
    }

    // Diff two commits
    async fn commit_compare(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitCompareParams,
    ) -> Result<thrift::CommitCompareResponse, service::CommitCompareExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self.repo(ctx, &commit.repo)?;

        let base_changeset_specifier = ChangesetSpecifier::from_request(&commit.id)?;
        let other_changeset_specifier = ChangesetSpecifier::from_request(&params.other_commit_id)?;
        let (base_changeset, other_changeset_id) = try_join!(
            repo.changeset(base_changeset_specifier),
            repo.resolve_specifier(other_changeset_specifier),
        )?;
        let base_changeset =
            base_changeset.ok_or_else(|| errors::commit_not_found(commit.description()))?;
        let other_changeset_id = other_changeset_id.ok_or_else(|| {
            errors::commit_not_found(format!(
                "repo={} commit={}",
                commit.repo.name,
                params.other_commit_id.to_string()
            ))
        })?;
        let diff = base_changeset.diff(other_changeset_id, true).await?;
        let diff_files = stream::iter(diff)
            .map(|d| d.into_response())
            .buffer_unordered(CONCURRENCY_LIMIT)
            .try_collect()
            .await?;

        Ok(thrift::CommitCompareResponse { diff_files })
    }

    /// Returns information about the file or directory at a path in a commit.
    async fn commit_path_info(
        &self,
        commit_path: thrift::CommitPathSpecifier,
        _params: thrift::CommitPathInfoParams,
    ) -> Result<thrift::CommitPathInfoResponse, service::CommitPathInfoExn> {
        let ctx = self.create_ctx(Some(&commit_path));
        let (_repo, changeset) = self.repo_changeset(ctx, &commit_path.commit).await?;
        let path = changeset.path(&commit_path.path)?;
        let response = match path.entry().await? {
            PathEntry::NotPresent => thrift::CommitPathInfoResponse {
                exists: false,
                type_: None,
                info: None,
            },
            PathEntry::Tree(tree) => {
                let summary = tree.summary().await?;
                let tree_info = thrift::TreeInfo {
                    id: tree.id().as_ref().to_vec(),
                    simple_format_sha1: summary.simple_format_sha1.as_ref().to_vec(),
                    simple_format_sha256: summary.simple_format_sha256.as_ref().to_vec(),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                };
                thrift::CommitPathInfoResponse {
                    exists: true,
                    type_: Some(thrift::EntryType::TREE),
                    info: Some(thrift::EntryInfo::tree(tree_info)),
                }
            }
            PathEntry::File(file, file_type) => {
                let metadata = file.metadata().await?;
                let file_info = thrift::FileInfo {
                    id: metadata.content_id.as_ref().to_vec(),
                    file_size: metadata.total_size as i64,
                    content_sha1: metadata.sha1.as_ref().to_vec(),
                    content_sha256: metadata.sha256.as_ref().to_vec(),
                };
                thrift::CommitPathInfoResponse {
                    exists: true,
                    type_: Some(file_type.into_response()),
                    info: Some(thrift::EntryInfo::file(file_info)),
                }
            }
        };
        Ok(response)
    }

    /// List the contents of a directory.
    async fn tree_list(
        &self,
        tree: thrift::TreeSpecifier,
        params: thrift::TreeListParams,
    ) -> Result<thrift::TreeListResponse, service::TreeListExn> {
        let ctx = self.create_ctx(Some(&tree));
        let (_repo, tree) = self.repo_tree(ctx, &tree).await?;
        if let Some(tree) = tree {
            let summary = tree.summary().await?;
            let entries = tree
                .list()
                .await?
                .skip(params.offset as usize)
                .take(params.limit as usize)
                .map(IntoResponse::into_response)
                .collect();
            let response = thrift::TreeListResponse {
                entries,
                count: (summary.child_files_count + summary.child_dirs_count) as i64,
            };
            Ok(response)
        } else {
            // Listing a path that is not a directory just returns an empty list.
            Ok(thrift::TreeListResponse {
                entries: Vec::new(),
                count: 0,
            })
        }
    }

    /// Test whether a file exists.
    async fn file_exists(
        &self,
        file: thrift::FileSpecifier,
        _params: thrift::FileExistsParams,
    ) -> Result<bool, service::FileExistsExn> {
        let ctx = self.create_ctx(Some(&file));
        let (_repo, file) = self.repo_file(ctx, &file).await?;
        Ok(file.is_some())
    }

    /// Get file info.
    async fn file_info(
        &self,
        file: thrift::FileSpecifier,
        _params: thrift::FileInfoParams,
    ) -> Result<thrift::FileInfo, service::FileInfoExn> {
        let ctx = self.create_ctx(Some(&file));
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => Ok(file.metadata().await?.into_response()),
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Get a chunk of file content.
    async fn file_content_chunk(
        &self,
        file: thrift::FileSpecifier,
        params: thrift::FileContentChunkParams,
    ) -> Result<thrift::FileChunk, service::FileContentChunkExn> {
        let ctx = self.create_ctx(Some(&file));
        let offset: u64 = check_range_and_convert("offset", params.offset, 0..)?;
        let size: u64 = check_range_and_convert("size", params.size, 0..=MAX_CHUNK_SIZE)?;
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => {
                let metadata = file.metadata().await?;
                let expected_size = min(size, metadata.total_size.saturating_sub(offset));
                let mut data = Vec::with_capacity(expected_size as usize);
                file.content_range(offset, size)
                    .await
                    .for_each(|bytes| {
                        data.put(bytes);
                        Ok(())
                    })
                    .compat()
                    .await
                    .map_err(errors::internal_error)?;
                Ok(thrift::FileChunk {
                    offset: params.offset,
                    file_size: metadata.total_size as i64,
                    data,
                })
            }
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Do a cross-repo lookup to see if a commit exists under a different hash in another repo
    async fn commit_lookup_xrepo(
        &self,
        commit: thrift::CommitSpecifier,
        params: thrift::CommitLookupXRepoParams,
    ) -> Result<thrift::CommitLookupResponse, service::CommitLookupXrepoExn> {
        let ctx = self.create_ctx(Some(&commit));
        let repo = self.repo(ctx.clone(), &commit.repo)?;
        let other_repo = self.repo(ctx, &params.other_repo)?;
        match repo
            .xrepo_commit_lookup(&other_repo, ChangesetSpecifier::from_request(&commit.id)?)
            .await?
        {
            Some(cs) => {
                let ids = map_commit_identity(&cs, &params.identity_schemes).await?;
                Ok(thrift::CommitLookupResponse {
                    exists: true,
                    ids: Some(ids),
                })
            }
            None => Ok(thrift::CommitLookupResponse {
                exists: false,
                ids: None,
            }),
        }
    }
}
