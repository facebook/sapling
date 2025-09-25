/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use anyhow::Error;
use bytes::Bytes;
use context::CoreContext;
use futures::try_join;
use mononoke_api::MononokeRepo;
use mononoke_api::RepoContext;

use crate::types::DiffSingleInput;
use crate::types::HeaderlessDiffOpts;
use crate::types::HeaderlessUnifiedDiff;
use crate::utils::content::load_content;

pub async fn headerless_unified<R: MononokeRepo>(
    ctx: &CoreContext,
    repo: &RepoContext<R>,
    base: DiffSingleInput,
    other: DiffSingleInput,
    context: usize,
) -> Result<HeaderlessUnifiedDiff, Error> {
    let (base_bytes, other_bytes) = try_join!(
        load_content(ctx, repo, base),
        load_content(ctx, repo, other)
    )?;

    let base_content = base_bytes.unwrap_or_else(Bytes::new);
    let other_content = other_bytes.unwrap_or_else(Bytes::new);

    let is_binary = xdiff::file_is_binary(&Some(xdiff::DiffFile {
        path: "base".to_string(),
        contents: xdiff::FileContent::Inline(base_content.clone()),
        file_type: xdiff::FileType::Regular,
    })) || xdiff::file_is_binary(&Some(xdiff::DiffFile {
        path: "other".to_string(),
        contents: xdiff::FileContent::Inline(other_content.clone()),
        file_type: xdiff::FileType::Regular,
    }));

    let opts = HeaderlessDiffOpts { context };
    let xdiff_opts = xdiff::HeaderlessDiffOpts::from(opts);

    let raw_diff = if is_binary {
        b"Binary files differ\n".to_vec()
    } else {
        xdiff::diff_unified_headerless(&other_content, &base_content, xdiff_opts)
    };

    Ok(HeaderlessUnifiedDiff {
        raw_diff,
        is_binary,
    })
}
