/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Result;
use anyhow::anyhow;
use futures::try_join;
use mononoke_api::ChangesetPathContentContext;
use mononoke_api::CopyInfo;
use mononoke_api::Repo;
use source_control as source_control_thrift;

/// Convert a mononoke_api FileType to the thrift EntryType.
pub fn convert_file_type(file_type: mononoke_api::FileType) -> source_control_thrift::EntryType {
    match file_type {
        mononoke_api::FileType::Regular => source_control_thrift::EntryType::FILE,
        mononoke_api::FileType::Executable => source_control_thrift::EntryType::EXEC,
        mononoke_api::FileType::Symlink => source_control_thrift::EntryType::LINK,
        mononoke_api::FileType::GitSubmodule => source_control_thrift::EntryType::GIT_SUBMODULE,
    }
}

/// Convert a mononoke_api FileMetadata to the thrift FileInfo.
pub fn convert_file_metadata(meta: mononoke_api::FileMetadata) -> source_control_thrift::FileInfo {
    source_control_thrift::FileInfo {
        id: meta.content_id.as_ref().to_vec(),
        file_size: meta.total_size as i64,
        content_sha1: meta.sha1.as_ref().to_vec(),
        content_sha256: meta.sha256.as_ref().to_vec(),
        content_git_sha1: meta.git_sha1.as_ref().to_vec(),
        content_seeded_blake3: meta.seeded_blake3.as_ref().to_vec(),
        is_binary: meta.is_binary,
        is_ascii: meta.is_ascii,
        is_utf8: meta.is_utf8,
        ends_in_newline: meta.ends_in_newline,
        newline_count: meta.newline_count as i64,
        first_line: meta.first_line,
        is_generated: meta.is_generated,
        is_partially_generated: meta.is_partially_generated,
        ..Default::default()
    }
}

/// Convert a mononoke_api CopyInfo to the thrift CopyInfo.
pub fn convert_copy_info(copy_info: CopyInfo) -> source_control_thrift::CopyInfo {
    match copy_info {
        CopyInfo::None => source_control_thrift::CopyInfo::NONE,
        CopyInfo::Copy => source_control_thrift::CopyInfo::COPY,
        CopyInfo::Move => source_control_thrift::CopyInfo::MOVE,
    }
}

/// Convert a ChangesetPathContentContext to a thrift FilePathInfo.
pub async fn to_file_path_info(
    ctx: Option<&ChangesetPathContentContext<Repo>>,
) -> Result<Option<source_control_thrift::FilePathInfo>> {
    match ctx {
        None => Ok(None),
        Some(ctx) => {
            let (meta, file_type) = try_join!(
                async {
                    Ok::<_, anyhow::Error>(
                        ctx.file()
                            .await?
                            .ok_or_else(|| anyhow!("programming error: not a file"))?
                            .metadata()
                            .await?,
                    )
                },
                async {
                    ctx.file_type()
                        .await?
                        .ok_or_else(|| anyhow!("programming error: not a file"))
                },
            )?;
            Ok(Some(source_control_thrift::FilePathInfo {
                path: ctx.path().to_string(),
                r#type: convert_file_type(file_type),
                info: convert_file_metadata(meta),
                ..Default::default()
            }))
        }
    }
}

/// Convert a ChangesetPathContentContext to a thrift TreePathInfo.
pub async fn to_tree_path_info(
    ctx: Option<&ChangesetPathContentContext<Repo>>,
) -> Result<Option<source_control_thrift::TreePathInfo>> {
    match ctx {
        None => Ok(None),
        Some(ctx) => {
            let tree = ctx
                .tree()
                .await?
                .ok_or_else(|| anyhow!("programming error: not a tree"))?;
            let summary = tree.summary().await?;
            Ok(Some(source_control_thrift::TreePathInfo {
                path: ctx.path().to_string(),
                info: source_control_thrift::TreeInfo {
                    id: tree.id().as_ref().to_vec(),
                    child_files_count: summary.child_files_count as i64,
                    child_files_total_size: summary.child_files_total_size as i64,
                    child_dirs_count: summary.child_dirs_count as i64,
                    descendant_files_count: summary.descendant_files_count as i64,
                    descendant_files_total_size: summary.descendant_files_total_size as i64,
                    ..Default::default()
                },
                ..Default::default()
            }))
        }
    }
}
