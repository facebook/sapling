/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashSet;

use async_trait::async_trait;
use commit_cloud_types::WorkspaceData;
use futures::future::try_join_all;
use futures::try_join;
use itertools::Itertools;
use maplit::btreemap;
use mononoke_api::BookmarkInfo;
use mononoke_api::ChangesetContext;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetPathContentContext;
use mononoke_api::CopyInfo;
use mononoke_api::FileContentType;
use mononoke_api::FileGeneratedStatus;
use mononoke_api::FileMetadata;
use mononoke_api::FileType;
use mononoke_api::HeaderlessUnifiedDiff;
use mononoke_api::MetadataDiff;
use mononoke_api::MetadataDiffFileInfo;
use mononoke_api::MetadataDiffLinesCount;
use mononoke_api::MononokeError;
use mononoke_api::PushrebaseOutcome;
use mononoke_api::Repo;
use mononoke_api::RepoContext;
use mononoke_api::TreeEntry;
use mononoke_api::TreeId;
use mononoke_api::TreeSummary;
use mononoke_api::UnifiedDiff;
use mononoke_types::MPath;
use mononoke_types::SubtreeChange;
use mononoke_types::subtree_change::SubtreeCopy;
use mononoke_types::subtree_change::SubtreeDeepCopy;
use mononoke_types::subtree_change::SubtreeImport;
use mononoke_types::subtree_change::SubtreeMerge;
use sorted_vector_map::SortedVectorMap;
use source_control as thrift;

use crate::commit_id::map_commit_identities;
use crate::commit_id::map_commit_identity;

/// Convert an item into a thrift type suitable for inclusion in a thrift
/// response.
pub(crate) trait IntoResponse<T> {
    fn into_response(self) -> T;
}

/// Asynchronously convert an item into a thrift type suitable for inclusion
/// in a thrift response.
#[async_trait]
pub(crate) trait AsyncIntoResponse<T> {
    async fn into_response(self) -> Result<T, scs_errors::ServiceError>;
}

/// Asynchronously convert an item into a thrift type suitable for inclusion
/// in a thrift response, with additional data required for the conversion.
#[async_trait]
pub(crate) trait AsyncIntoResponseWith<T> {
    /// The type of additional data that must be provided to convert this
    /// value into a response value.
    type Additional;

    async fn into_response_with(
        self,
        additional: &Self::Additional,
    ) -> Result<T, scs_errors::ServiceError>;
}

#[async_trait]
impl<T, A: AsyncIntoResponse<T> + Send> AsyncIntoResponse<Option<T>> for Option<A> {
    async fn into_response(self) -> Result<Option<T>, scs_errors::ServiceError> {
        match self {
            Some(value) => Ok(Some(value.into_response().await?)),
            None => Ok(None),
        }
    }
}

impl IntoResponse<thrift::EntryType> for FileType {
    fn into_response(self) -> thrift::EntryType {
        match self {
            FileType::Regular => thrift::EntryType::FILE,
            FileType::Executable => thrift::EntryType::EXEC,
            FileType::Symlink => thrift::EntryType::LINK,
            FileType::GitSubmodule => thrift::EntryType::GIT_SUBMODULE,
        }
    }
}

impl IntoResponse<Option<thrift::MetadataDiffFileType>> for Option<FileType> {
    fn into_response(self) -> Option<thrift::MetadataDiffFileType> {
        match self {
            None => None,
            Some(FileType::Regular) => Some(thrift::MetadataDiffFileType::FILE),
            Some(FileType::Executable) => Some(thrift::MetadataDiffFileType::EXEC),
            Some(FileType::Symlink) => Some(thrift::MetadataDiffFileType::LINK),
            Some(FileType::GitSubmodule) => Some(thrift::MetadataDiffFileType::GIT_SUBMODULE),
        }
    }
}

impl IntoResponse<thrift::TreeEntry> for (String, TreeEntry) {
    fn into_response(self) -> thrift::TreeEntry {
        let (name, entry) = self;
        let (r#type, info) = match entry {
            TreeEntry::Directory(dir) => {
                let summary = dir.summary();
                let info = thrift::TreeInfo {
                    id: dir.id().as_ref().to_vec(),
                    simple_format_sha1: Some(summary.simple_format_sha1.as_ref().to_vec()),
                    simple_format_sha256: Some(summary.simple_format_sha256.as_ref().to_vec()),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                    ..Default::default()
                };
                (thrift::EntryType::TREE, thrift::EntryInfo::tree(info))
            }
            TreeEntry::File(file) => {
                let info = thrift::FileInfo {
                    id: file.content_id().as_ref().to_vec(),
                    file_size: file.size() as i64,
                    content_sha1: file.content_sha1().as_ref().to_vec(),
                    content_sha256: file.content_sha256().as_ref().to_vec(),
                    ..Default::default()
                };
                (
                    file.file_type().into_response(),
                    thrift::EntryInfo::file(info),
                )
            }
        };
        thrift::TreeEntry {
            name,
            r#type,
            info,
            ..Default::default()
        }
    }
}

impl IntoResponse<thrift::FileInfo> for FileMetadata {
    fn into_response(self) -> thrift::FileInfo {
        thrift::FileInfo {
            id: self.content_id.as_ref().to_vec(),
            file_size: self.total_size as i64,
            content_sha1: self.sha1.as_ref().to_vec(),
            content_sha256: self.sha256.as_ref().to_vec(),
            content_git_sha1: self.git_sha1.as_ref().to_vec(),
            content_seeded_blake3: self.seeded_blake3.as_ref().to_vec(),
            is_binary: self.is_binary,
            is_ascii: self.is_ascii,
            is_utf8: self.is_utf8,
            ends_in_newline: self.ends_in_newline,
            newline_count: self.newline_count as i64,
            first_line: self.first_line,
            is_generated: self.is_generated,
            is_partially_generated: self.is_partially_generated,
            ..Default::default()
        }
    }
}

impl IntoResponse<thrift::TreeInfo> for (TreeId, TreeSummary) {
    fn into_response(self) -> thrift::TreeInfo {
        let (id, summary) = self;
        thrift::TreeInfo {
            id: id.as_ref().to_vec(),
            simple_format_sha1: Some(summary.simple_format_sha1.as_ref().to_vec()),
            simple_format_sha256: Some(summary.simple_format_sha256.as_ref().to_vec()),
            child_files_count: summary.child_files_count as i64,
            child_files_total_size: summary.child_files_total_size as i64,
            child_dirs_count: summary.child_dirs_count as i64,
            descendant_files_count: summary.descendant_files_count as i64,
            descendant_files_total_size: summary.descendant_files_total_size as i64,
            ..Default::default()
        }
    }
}

impl IntoResponse<thrift::CopyInfo> for CopyInfo {
    fn into_response(self) -> thrift::CopyInfo {
        match self {
            CopyInfo::None => thrift::CopyInfo::NONE,
            CopyInfo::Copy => thrift::CopyInfo::COPY,
            CopyInfo::Move => thrift::CopyInfo::MOVE,
        }
    }
}

impl IntoResponse<Option<thrift::MetadataDiffFileContentType>> for Option<FileContentType> {
    fn into_response(self) -> Option<thrift::MetadataDiffFileContentType> {
        match self {
            None => None,
            Some(FileContentType::Text) => Some(thrift::MetadataDiffFileContentType::TEXT),
            Some(FileContentType::NonUtf8) => Some(thrift::MetadataDiffFileContentType::NON_UTF8),
            Some(FileContentType::Binary) => Some(thrift::MetadataDiffFileContentType::BINARY),
        }
    }
}

impl IntoResponse<Option<thrift::FileGeneratedStatus>> for Option<FileGeneratedStatus> {
    fn into_response(self) -> Option<thrift::FileGeneratedStatus> {
        match self {
            None => None,
            Some(FileGeneratedStatus::FullyGenerated) => {
                Some(thrift::FileGeneratedStatus::FULLY_GENERATED)
            }
            Some(FileGeneratedStatus::PartiallyGenerated) => {
                Some(thrift::FileGeneratedStatus::PARTIALLY_GENERATED)
            }
            Some(FileGeneratedStatus::NotGenerated) => {
                Some(thrift::FileGeneratedStatus::NOT_GENERATED)
            }
        }
    }
}

impl IntoResponse<thrift::MetadataDiffFileInfo> for MetadataDiffFileInfo {
    fn into_response(self) -> thrift::MetadataDiffFileInfo {
        thrift::MetadataDiffFileInfo {
            file_type: self.file_type.into_response(),
            file_content_type: self.file_content_type.into_response(),
            file_generated_status: self.file_generated_status.into_response(),
            ..Default::default()
        }
    }
}

impl IntoResponse<Option<thrift::MetadataDiffLinesCount>> for Option<MetadataDiffLinesCount> {
    fn into_response(self) -> Option<thrift::MetadataDiffLinesCount> {
        self.map(|lines_count| thrift::MetadataDiffLinesCount {
            added_lines_count: lines_count.added_lines_count as i64,
            deleted_lines_count: lines_count.deleted_lines_count as i64,
            significant_added_lines_count: lines_count.significant_added_lines_count as i64,
            significant_deleted_lines_count: lines_count.significant_deleted_lines_count as i64,
            first_added_line_number: lines_count
                .first_added_line_number
                .map(|line_number| line_number as i64),
            ..Default::default()
        })
    }
}

impl IntoResponse<thrift::Diff> for MetadataDiff {
    fn into_response(self) -> thrift::Diff {
        thrift::Diff::metadata_diff(thrift::MetadataDiff {
            old_file_info: self.old_file_info.into_response(),
            new_file_info: self.new_file_info.into_response(),
            lines_count: self.lines_count.into_response(),
            ..Default::default()
        })
    }
}

impl IntoResponse<thrift::Diff> for UnifiedDiff {
    fn into_response(self) -> thrift::Diff {
        thrift::Diff::raw_diff(thrift::RawDiff {
            raw_diff: Some(self.raw_diff),
            is_binary: self.is_binary,
            ..Default::default()
        })
    }
}

impl IntoResponse<thrift::Diff> for HeaderlessUnifiedDiff {
    fn into_response(self) -> thrift::Diff {
        thrift::Diff::raw_diff(thrift::RawDiff {
            raw_diff: Some(self.raw_diff),
            is_binary: self.is_binary,
            ..Default::default()
        })
    }
}

impl IntoResponse<thrift::WorkspaceInfo> for WorkspaceData {
    fn into_response(self) -> thrift::WorkspaceInfo {
        thrift::WorkspaceInfo {
            specifier: thrift::WorkspaceSpecifier {
                name: self.name,
                repo: thrift::RepoSpecifier {
                    name: self.reponame,
                    ..Default::default()
                },
                ..Default::default()
            },
            is_archived: self.archived,
            latest_version: self.version as i64,
            latest_timestamp: self.timestamp,
            ..Default::default()
        }
    }
}

#[async_trait]
impl AsyncIntoResponse<thrift::FilePathInfo> for &ChangesetPathContentContext<Repo> {
    async fn into_response(self) -> Result<thrift::FilePathInfo, scs_errors::ServiceError> {
        let (meta, type_) = try_join!(
            async {
                Ok::<_, scs_errors::ServiceError>(
                    self.file()
                        .await?
                        .ok_or_else(|| scs_errors::internal_error("programming error: not a file"))?
                        .metadata()
                        .await?,
                )
            },
            async {
                Ok::<_, scs_errors::ServiceError>(
                    self.file_type().await?.ok_or_else(|| {
                        scs_errors::internal_error("programming error: not a file")
                    })?,
                )
            },
        )?;
        Ok(thrift::FilePathInfo {
            path: self.path().to_string(),
            r#type: type_.into_response(),
            info: meta.into_response(),
            ..Default::default()
        })
    }
}

#[async_trait]
impl AsyncIntoResponse<thrift::TreePathInfo> for &ChangesetPathContentContext<Repo> {
    async fn into_response(self) -> Result<thrift::TreePathInfo, scs_errors::ServiceError> {
        let tree = self
            .tree()
            .await?
            .ok_or_else(|| scs_errors::internal_error("programming error: not a tree"))?;

        let summary = (tree.id().clone(), tree.summary().await?);
        Ok(thrift::TreePathInfo {
            path: self.path().to_string(),
            info: summary.into_response(),
            ..Default::default()
        })
    }
}

#[async_trait]
impl AsyncIntoResponseWith<thrift::CommitInfo> for ChangesetContext<Repo> {
    /// The additional data is the set of commit identity schemes to be
    /// returned in the response.
    type Additional = BTreeSet<thrift::CommitIdentityScheme>;

    async fn into_response_with(
        self,
        identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
    ) -> Result<thrift::CommitInfo, scs_errors::ServiceError> {
        async fn map_parent_identities(
            changeset: &ChangesetContext<Repo>,
            identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
        ) -> Result<Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>, MononokeError>
        {
            let parents = changeset.parents().await?;
            let parent_id_mapping =
                map_commit_identities(changeset.repo_ctx(), parents.clone(), identity_schemes)
                    .await?;
            Ok(parents
                .iter()
                .map(|parent_id| {
                    parent_id_mapping
                        .get(parent_id)
                        .cloned()
                        .unwrap_or_default()
                })
                .collect())
        }

        let (
            ids,
            message,
            date,
            author,
            parents,
            hg_extra,
            git_extra_headers,
            generation,
            committer_date,
            committer,
            linear_depth,
            subtree_change_count,
        ) = try_join!(
            map_commit_identity(&self, identity_schemes),
            self.message(),
            self.author_date(),
            self.author(),
            map_parent_identities(&self, identity_schemes),
            self.hg_extras(),
            self.git_extra_headers(),
            self.generation(),
            self.committer_date(),
            self.committer(),
            self.linear_depth(),
            self.subtree_change_count(),
        )?;
        Ok(thrift::CommitInfo {
            ids,
            message,
            date: date.timestamp(),
            tz: date.offset().local_minus_utc(),
            author,
            parents,
            extra: hg_extra.into_iter().collect(),
            git_extra_headers: git_extra_headers.map(|headers| {
                headers
                    .into_iter()
                    .map(|(k, v)| (thrift::small_binary(k), v))
                    .collect()
            }),
            generation: generation.value() as i64,
            committer_date: committer_date.map(|date| date.timestamp()),
            committer_tz: committer_date.map(|date| date.offset().local_minus_utc()),
            committer,
            linear_depth: Some(linear_depth as i64),
            subtree_change_count: Some(subtree_change_count as i64),
            ..Default::default()
        })
    }
}

#[async_trait]
impl AsyncIntoResponseWith<Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>>
    for Vec<ChangesetContext<Repo>>
{
    /// The additional data is the set of commit identity schemes to be
    /// returned in the response.
    type Additional = BTreeSet<thrift::CommitIdentityScheme>;

    async fn into_response_with(
        self,
        identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
    ) -> Result<
        Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
        scs_errors::ServiceError,
    > {
        let res = try_join_all({
            let changesets_grouped_by_repo = self
                .into_iter()
                .map(|c| c.into_repo_ctx_and_id())
                .into_group_map();

            changesets_grouped_by_repo
                .into_iter()
                .map(|(repo, changesets)| async move {
                    let id_map =
                        map_commit_identities(&repo, changesets.clone(), identity_schemes).await?;

                    changesets
                        .iter()
                        .map(move |id| {
                            id_map.get(id).cloned().ok_or_else(|| {
                                scs_errors::internal_error(
                                    "programming error, id is missing from the map",
                                )
                                .into()
                            })
                        })
                        .collect::<Result<
                            Vec<BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
                            scs_errors::ServiceError,
                        >>()
                })
        })
        .await?
        .into_iter()
        .flatten()
        .collect();
        Ok(res)
    }
}

fn to_i64(val: usize) -> Result<i64, scs_errors::ServiceError> {
    val.try_into()
        .map_err(|_| scs_errors::internal_error("usize too big for i64").into())
}

#[async_trait]
impl AsyncIntoResponseWith<thrift::PushrebaseOutcome> for PushrebaseOutcome {
    /// The additional data is the repo context, the set of commit identity
    /// schemes to be returned in the response, and optionally a different set
    /// of commit identity schemes to use for the old commit ids.
    type Additional = (
        RepoContext<Repo>,
        BTreeSet<thrift::CommitIdentityScheme>,
        Option<BTreeSet<thrift::CommitIdentityScheme>>,
    );

    async fn into_response_with(
        self,
        additional: &Self::Additional,
    ) -> Result<thrift::PushrebaseOutcome, scs_errors::ServiceError> {
        let (repo, identity_schemes, old_identity_schemes) = additional;
        let mut new_ids = HashSet::new();
        let mut old_ids = HashSet::new();
        new_ids.insert(self.head);
        for rebase in self.rebased_changesets.iter() {
            old_ids.insert(rebase.id_old);
            new_ids.insert(rebase.id_new);
        }
        let old_identity_schemes = old_identity_schemes.as_ref().unwrap_or(identity_schemes);
        let (old_id_map, new_id_map) = try_join!(
            map_commit_identities(repo, old_ids.into_iter().collect(), old_identity_schemes),
            map_commit_identities(repo, new_ids.into_iter().collect(), identity_schemes),
        )?;

        // Map IDs using one of the maps we just fetched.  If we couldn't
        // perform the look-up then just return the bonsai ID only.
        fn try_get(
            map: &BTreeMap<ChangesetId, BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId>>,
            cs_id: ChangesetId,
        ) -> BTreeMap<thrift::CommitIdentityScheme, thrift::CommitId> {
            match map.get(&cs_id) {
                Some(ids) => ids.clone(),
                None => btreemap! {
                    thrift::CommitIdentityScheme::BONSAI =>
                        thrift::CommitId::bonsai(cs_id.as_ref().into()),
                },
            }
        }

        let old_bookmark_value = self
            .old_bookmark_value
            .map_or_else(BTreeMap::new, |old_bookmark_value| {
                try_get(&new_id_map, old_bookmark_value)
            });
        let head = try_get(&new_id_map, self.head);
        let rebased_commits: Vec<_> = self
            .rebased_changesets
            .iter()
            .map(|rebase| {
                let old_ids = try_get(&old_id_map, rebase.id_old);
                let new_ids = try_get(&new_id_map, rebase.id_new);
                thrift::PushrebaseRebasedCommit {
                    old_ids,
                    new_ids,
                    ..Default::default()
                }
            })
            .collect();

        Ok(thrift::PushrebaseOutcome {
            old_bookmark_value,
            head,
            rebased_commits,
            pushrebase_distance: to_i64(self.pushrebase_distance.0)?,
            retry_num: to_i64(self.retry_num.0)?,
            ..Default::default()
        })
    }
}

#[async_trait]
impl AsyncIntoResponseWith<thrift::BookmarkInfo> for BookmarkInfo<Repo> {
    /// The additional data is the set of commit identity schemes to be
    /// returned in the response.
    type Additional = BTreeSet<thrift::CommitIdentityScheme>;

    async fn into_response_with(
        self,
        identity_schemes: &BTreeSet<thrift::CommitIdentityScheme>,
    ) -> Result<thrift::BookmarkInfo, scs_errors::ServiceError> {
        let (warm_ids, fresh_ids) = try_join!(
            map_commit_identity(&self.warm_changeset, identity_schemes),
            map_commit_identity(&self.fresh_changeset, identity_schemes),
        )?;
        Ok(thrift::BookmarkInfo {
            warm_ids,
            fresh_ids,
            last_update_timestamp_ns: self.last_update_timestamp.timestamp_nanos(),
            ..Default::default()
        })
    }
}

#[async_trait]
impl AsyncIntoResponseWith<BTreeMap<String, thrift::SubtreeChange>>
    for SortedVectorMap<MPath, SubtreeChange>
{
    /// The additional data is the repo context and the set of commit identity
    /// schemes to be returned in the response.
    type Additional = (RepoContext<Repo>, BTreeSet<thrift::CommitIdentityScheme>);

    async fn into_response_with(
        self,
        additional: &Self::Additional,
    ) -> Result<BTreeMap<String, thrift::SubtreeChange>, scs_errors::ServiceError> {
        let (repo, identity_schemes) = additional;
        let cs_ids: HashSet<_> = self
            .iter()
            .flat_map(|(_, change)| change.source())
            .collect();
        let id_map =
            map_commit_identities(repo, cs_ids.into_iter().collect(), identity_schemes).await?;

        self.into_iter()
            .map(|(path, change)| {
                let change = match change {
                    SubtreeChange::SubtreeCopy(SubtreeCopy {
                        from_cs_id,
                        from_path,
                    })
                    | SubtreeChange::SubtreeDeepCopy(SubtreeDeepCopy {
                        from_cs_id,
                        from_path,
                    }) => thrift::SubtreeChange::subtree_copy(thrift::SubtreeCopy {
                        source_path: from_path.to_string(),
                        source_commit_ids: id_map.get(&from_cs_id).cloned().unwrap_or_default(),
                        ..Default::default()
                    }),
                    SubtreeChange::SubtreeMerge(SubtreeMerge {
                        from_cs_id,
                        from_path,
                    }) => thrift::SubtreeChange::subtree_merge(thrift::SubtreeMerge {
                        source_path: from_path.to_string(),
                        source_commit_ids: id_map.get(&from_cs_id).cloned().unwrap_or_default(),
                        ..Default::default()
                    }),
                    SubtreeChange::SubtreeImport(SubtreeImport {
                        from_path,
                        from_commit,
                        from_repo_url,
                    }) => thrift::SubtreeChange::subtree_import(thrift::SubtreeImport {
                        source_path: from_path.to_string(),
                        source_commit_id: from_commit,
                        source_url: from_repo_url,
                        ..Default::default()
                    }),
                };
                Ok((path.to_string(), change))
            })
            .collect()
    }
}
