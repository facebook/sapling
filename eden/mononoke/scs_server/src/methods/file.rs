/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use context::CoreContext;
use source_control as thrift;

use crate::errors;
use crate::from_request::check_range_and_convert;
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
                })
            }
            (_repo, None) => Err(errors::file_not_found(file.description()).into()),
        }
    }
}
