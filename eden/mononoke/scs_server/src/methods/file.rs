/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use mononoke_api::headerless_unified_diff;
use mononoke_api::FileId;
use source_control as thrift;

use crate::errors;
use crate::errors::ServiceErrorResultExt;
use crate::from_request::check_range_and_convert;
use crate::from_request::FromRequest;
use crate::into_response::IntoResponse;
use crate::source_control_impl::SourceControlServiceImpl;
use crate::specifiers::SpecifierExt;

impl SourceControlServiceImpl {
    /// Test whether a file exists.
    pub(crate) async fn file_exists(
        &self,
        ctx: CoreContext,
        file: thrift::FileSpecifier,
        _params: thrift::FileExistsParams,
    ) -> Result<bool, errors::ServiceError> {
        let (_repo, file) = self.repo_file(ctx, &file).await?;
        Ok(file.is_some())
    }

    /// Get file info.
    pub(crate) async fn file_info(
        &self,
        ctx: CoreContext,
        file: thrift::FileSpecifier,
        _params: thrift::FileInfoParams,
    ) -> Result<thrift::FileInfo, errors::ServiceError> {
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => Ok(file.metadata().await?.into_response()),
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Get a chunk of file content.
    pub(crate) async fn file_content_chunk(
        &self,
        ctx: CoreContext,
        file: thrift::FileSpecifier,
        params: thrift::FileContentChunkParams,
    ) -> Result<thrift::FileChunk, errors::ServiceError> {
        let offset: u64 = check_range_and_convert("offset", params.offset, 0..)?;
        let size: u64 = check_range_and_convert(
            "size",
            params.size,
            0..=source_control::FILE_CONTENT_CHUNK_SIZE_LIMIT,
        )?;
        match self.repo_file(ctx, &file).await? {
            (_repo, Some(file)) => {
                let metadata = file.metadata().await?;
                let data = file.content_range_concat(offset, size).await?;
                Ok(thrift::FileChunk {
                    offset: params.offset,
                    file_size: metadata.total_size as i64,
                    data: Vec::from(data.as_ref()),
                    ..Default::default()
                })
            }
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }

    /// Compare a file with another file.
    pub(crate) async fn file_diff(
        &self,
        ctx: CoreContext,
        file: thrift::FileSpecifier,
        params: thrift::FileDiffParams,
    ) -> Result<thrift::FileDiffResponse, errors::ServiceError> {
        let context_lines = params.context as usize;

        let (repo, base_file) = self.repo_file(ctx, &file).await?;
        let base_file = base_file
            .ok_or_else(|| errors::file_not_found(file.description()))
            .context("failed to resolve target file")?;

        let other_file_id = FileId::from_request(&params.other_file_id)?;
        let other_file = repo
            .file(other_file_id)
            .await?
            .ok_or_else(|| errors::file_not_found(other_file_id.to_string()))
            .context("failed to resolve other file")?;

        let diff = headerless_unified_diff(&other_file, &base_file, context_lines)
            .await?
            .into_response();

        Ok(thrift::FileDiffResponse {
            diff,
            ..Default::default()
        })
    }
}
