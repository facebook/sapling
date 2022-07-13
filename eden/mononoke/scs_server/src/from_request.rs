/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::ops::RangeBounds;
use std::str::FromStr;

use bookmarks_movement::BookmarkKindRestrictions;
use bytes::Bytes;
use chrono::DateTime;
use chrono::FixedOffset;
use chrono::TimeZone;
use ephemeral_blobstore::BubbleId;
use faster_hex::hex_string;
use hooks::CrossRepoPushSource;
use mononoke_api::specifiers::GitSha1;
use mononoke_api::specifiers::Globalrev;
use mononoke_api::specifiers::Svnrev;
use mononoke_api::BookmarkName;
use mononoke_api::CandidateSelectionHintArgs;
use mononoke_api::ChangesetId;
use mononoke_api::ChangesetIdPrefix;
use mononoke_api::ChangesetPrefixSpecifier;
use mononoke_api::ChangesetSpecifier;
use mononoke_api::CopyInfo;
use mononoke_api::CreateCopyInfo;
use mononoke_api::FileId;
use mononoke_api::FileType;
use mononoke_api::HgChangesetId;
use mononoke_api::HgChangesetIdPrefix;
use mononoke_api::MononokePath;
use mononoke_api::TreeId;
use mononoke_types::hash::Sha1;
use mononoke_types::hash::Sha256;
use source_control as thrift;

use crate::commit_id::CommitIdExt;
use crate::errors;

pub(crate) trait FromRequest<T: ?Sized> {
    fn from_request(t: &T) -> Result<Self, thrift::RequestError>
    where
        Self: Sized;
}

impl FromRequest<str> for BookmarkName {
    fn from_request(bookmark: &str) -> Result<BookmarkName, thrift::RequestError> {
        BookmarkName::new(bookmark).map_err(|e| {
            errors::invalid_request(format!(
                "failed parsing bookmark out of {}: {:?}",
                bookmark, e
            ))
        })
    }
}

impl FromRequest<thrift::CrossRepoPushSource> for CrossRepoPushSource {
    fn from_request(
        push_source: &thrift::CrossRepoPushSource,
    ) -> Result<Self, thrift::RequestError> {
        match push_source {
            &thrift::CrossRepoPushSource::NATIVE_TO_THIS_REPO => Ok(Self::NativeToThisRepo),
            &thrift::CrossRepoPushSource::PUSH_REDIRECTED => Ok(Self::PushRedirected),
            other => Err(errors::invalid_request(format!(
                "Unknown CrossRepoPushSource: {}",
                other
            ))),
        }
    }
}

impl FromRequest<thrift::BookmarkKindRestrictions> for BookmarkKindRestrictions {
    fn from_request(
        push_source: &thrift::BookmarkKindRestrictions,
    ) -> Result<Self, thrift::RequestError> {
        match push_source {
            &thrift::BookmarkKindRestrictions::ANY_KIND => Ok(Self::AnyKind),
            &thrift::BookmarkKindRestrictions::ONLY_SCRATCH => Ok(Self::OnlyScratch),
            &thrift::BookmarkKindRestrictions::ONLY_PUBLISHING => Ok(Self::OnlyPublishing),
            other => Err(errors::invalid_request(format!(
                "Unknown BookmarkKindRestrictions: {}",
                other
            ))),
        }
    }
}

impl FromRequest<thrift::CandidateSelectionHint> for CandidateSelectionHintArgs {
    fn from_request(hint: &thrift::CandidateSelectionHint) -> Result<Self, thrift::RequestError> {
        match hint {
            thrift::CandidateSelectionHint::bookmark_ancestor(bookmark) => {
                let bookmark = BookmarkName::from_request(bookmark)?;
                Ok(CandidateSelectionHintArgs::OnlyOrAncestorOfBookmark(
                    bookmark,
                ))
            }
            thrift::CandidateSelectionHint::bookmark_descendant(bookmark) => {
                let bookmark = BookmarkName::from_request(bookmark)?;
                Ok(CandidateSelectionHintArgs::OnlyOrDescendantOfBookmark(
                    bookmark,
                ))
            }
            thrift::CandidateSelectionHint::commit_ancestor(commit) => {
                let changeset_specifier = ChangesetSpecifier::from_request(commit)?;
                Ok(CandidateSelectionHintArgs::OnlyOrAncestorOfCommit(
                    changeset_specifier,
                ))
            }
            thrift::CandidateSelectionHint::commit_descendant(commit) => {
                let changeset_specifier = ChangesetSpecifier::from_request(commit)?;
                Ok(CandidateSelectionHintArgs::OnlyOrDescendantOfCommit(
                    changeset_specifier,
                ))
            }
            thrift::CandidateSelectionHint::exact(commit) => {
                let changeset_specifier = ChangesetSpecifier::from_request(commit)?;
                Ok(CandidateSelectionHintArgs::Exact(changeset_specifier))
            }
            thrift::CandidateSelectionHint::UnknownField(f) => Err(errors::invalid_request(
                format!("unsupported candidate selection hint: {:?}", f),
            )),
        }
    }
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
                        e
                    ))
                })?;
                Ok(ChangesetSpecifier::Bonsai(cs_id))
            }
            thrift::CommitId::hg(id) => {
                let hg_cs_id = HgChangesetId::from_bytes(id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e
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
            thrift::CommitId::git(git_sha1) => {
                let git_sha1 = GitSha1::from_bytes(&git_sha1).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e
                    ))
                })?;
                Ok(ChangesetSpecifier::GitSha1(git_sha1))
            }
            thrift::CommitId::svnrev(rev) => {
                let rev = Svnrev::new((*rev).try_into().map_err(|_| {
                    errors::invalid_request(format!("cannot parse svn revision {} to u64", rev))
                })?);
                Ok(ChangesetSpecifier::Svnrev(rev))
            }
            thrift::CommitId::ephemeral_bonsai(ephemeral) => {
                let cs_id = ChangesetId::from_bytes(&ephemeral.bonsai_id).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id (scheme={} {}): {}",
                        commit.scheme(),
                        commit.to_string(),
                        e
                    ))
                })?;
                let bubble_id = if ephemeral.bubble_id == 0 {
                    None
                } else {
                    Some(BubbleId::try_from(ephemeral.bubble_id).map_err(|_| {
                        errors::invalid_request(format!(
                            "invalid bubble id {}",
                            ephemeral.bubble_id
                        ))
                    })?)
                };
                Ok(ChangesetSpecifier::EphemeralBonsai(cs_id, bubble_id))
            }
            thrift::CommitId::UnknownField(_) => Err(errors::invalid_request(format!(
                "unsupported commit identity scheme ({})",
                commit.scheme()
            ))),
        }
    }
}

impl FromRequest<thrift::CopyInfo> for CopyInfo {
    fn from_request(copy_info: &thrift::CopyInfo) -> Result<Self, thrift::RequestError> {
        match *copy_info {
            thrift::CopyInfo::NONE => Ok(CopyInfo::None),
            thrift::CopyInfo::COPY => Ok(CopyInfo::Copy),
            thrift::CopyInfo::MOVE => Ok(CopyInfo::Move),
            val => Err(errors::invalid_request(format!(
                "unsupported copy info ({})",
                val
            ))),
        }
    }
}

impl FromRequest<thrift::RepoResolveCommitPrefixParams> for ChangesetPrefixSpecifier {
    fn from_request(
        params: &thrift::RepoResolveCommitPrefixParams,
    ) -> Result<Self, thrift::RequestError> {
        match params.prefix_scheme {
            thrift::CommitIdentityScheme::HG => {
                let prefix = HgChangesetIdPrefix::from_str(&params.prefix).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id prefix (scheme={} {}): {}",
                        params.prefix_scheme, params.prefix, e
                    ))
                })?;
                Ok(ChangesetPrefixSpecifier::from(prefix))
            }
            thrift::CommitIdentityScheme::BONSAI => {
                let prefix = ChangesetIdPrefix::from_str(&params.prefix).map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id prefix (scheme={} {}): {}",
                        params.prefix_scheme, params.prefix, e
                    ))
                })?;
                Ok(ChangesetPrefixSpecifier::from(prefix))
            }
            thrift::CommitIdentityScheme::GLOBALREV => {
                let rev = params.prefix.parse().map_err(|e| {
                    errors::invalid_request(format!(
                        "invalid commit id prefix (scheme={} {}): {}",
                        params.prefix_scheme, params.prefix, e
                    ))
                })?;
                Ok(ChangesetPrefixSpecifier::from(Globalrev::new(rev)))
            }
            _ => Err(errors::invalid_request(format!(
                "unsupported prefix identity scheme ({})",
                params.prefix_scheme
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
                        hex_string(&id),
                        e,
                    ))})
            }
        }
    }
);

impl_from_request_binary_id!(TreeId, "tree id");
impl_from_request_binary_id!(FileId, "file id");
impl_from_request_binary_id!(Sha1, "sha-1");
impl_from_request_binary_id!(Sha256, "sha-256");
impl_from_request_binary_id!(GitSha1, "git-sha-1");

impl FromRequest<thrift::RepoCreateCommitParamsFileType> for FileType {
    fn from_request(
        file_type: &thrift::RepoCreateCommitParamsFileType,
    ) -> Result<Self, thrift::RequestError> {
        match *file_type {
            thrift::RepoCreateCommitParamsFileType::FILE => Ok(FileType::Regular),
            thrift::RepoCreateCommitParamsFileType::EXEC => Ok(FileType::Executable),
            thrift::RepoCreateCommitParamsFileType::LINK => Ok(FileType::Symlink),
            val => Err(errors::invalid_request(format!(
                "unsupported file type ({})",
                val
            ))),
        }
    }
}

impl FromRequest<thrift::RepoCreateCommitParamsFileCopyInfo> for CreateCopyInfo {
    fn from_request(
        copy_info: &thrift::RepoCreateCommitParamsFileCopyInfo,
    ) -> Result<Self, thrift::RequestError> {
        let path = MononokePath::try_from(&copy_info.path).map_err(|e| {
            errors::invalid_request(format!(
                "invalid copy-from path '{}': {}",
                copy_info.path, e
            ))
        })?;
        let parent_index = usize::try_from(copy_info.parent_index).map_err(|e| {
            errors::invalid_request(format!(
                "invalid copy-from parent index '{}': {}",
                copy_info.parent_index, e
            ))
        })?;
        Ok(CreateCopyInfo::new(path, parent_index))
    }
}

impl FromRequest<thrift::DateTime> for DateTime<FixedOffset> {
    fn from_request(date: &thrift::DateTime) -> Result<Self, thrift::RequestError> {
        Ok(FixedOffset::east(date.tz).timestamp(date.timestamp, 0))
    }
}

/// Check that an input value is in range for the request, and convert it to
/// the internal type.  Returns a invalid request error if the number was out
/// of range, and an internal error if the conversion failed.
pub(crate) fn check_range_and_convert<F, T, B>(
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

pub(crate) fn validate_timestamp(
    ts: Option<i64>,
    name: &str,
) -> Result<Option<i64>, errors::ServiceError> {
    match ts {
        None | Some(0) => Ok(None),
        Some(ts) if ts < 0 => {
            Err(errors::invalid_request(format!("{} ({}) cannot be negative", name, ts)).into())
        }
        Some(ts) => Ok(Some(ts)),
    }
}

/// Convert a pushvars map from thrift's representation to the one used
/// internally in mononoke.
pub(crate) fn convert_pushvars(
    pushvars: Option<BTreeMap<String, Vec<u8>>>,
) -> Option<HashMap<String, Bytes>> {
    pushvars.map(|pushvars| {
        pushvars
            .into_iter()
            .map(|(name, value)| (name, Bytes::from(value)))
            .collect()
    })
}
