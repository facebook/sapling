/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::{format_err, Context, Result};
use futures::future::{FutureExt, TryFutureExt};
use futures::stream::{self, StreamExt, TryStreamExt};

use bookmarks::{
    BookmarkKind, BookmarkName, BookmarkPagination, BookmarkPrefix, Bookmarks, Freshness,
};
use context::CoreContext;
use mononoke_types::ChangesetId;

use crate::dag::{NameDagBuilder, VertexListWithOptions, VertexName, VertexOptions};
use crate::idmap::{vertex_name_from_cs_id, IdMap, IdMapWrapper};
use crate::{Group, InProcessIdDag};

pub type ServerNameDag = crate::dag::namedag::AbstractNameDag<InProcessIdDag, IdMapWrapper, (), ()>;

/// Convert a server IdDag and IdMap to a NameDag
/// Note: you will need to call NameDag::map().flush_writes
/// to write out updates to the IdMap
pub fn server_namedag(
    ctx: CoreContext,
    iddag: InProcessIdDag,
    idmap: Arc<dyn IdMap>,
) -> Result<ServerNameDag> {
    let idmap = IdMapWrapper::new(ctx, idmap);
    NameDagBuilder::new_with_idmap_dag(idmap, iddag)
        .build()
        .map_err(anyhow::Error::from)
}

pub fn head_with_options(head: ChangesetId) -> (VertexName, VertexOptions) {
    let mut options = VertexOptions::default();
    options.reserve_size = 1 << 26;
    options.highest_group = Group::MASTER;
    (vertex_name_from_cs_id(&head), options)
}

pub async fn bookmark_with_options(
    ctx: &CoreContext,
    bookmark: Option<&BookmarkName>,
    bookmarks: &dyn Bookmarks,
) -> Result<VertexListWithOptions> {
    let bm_stream = match bookmark {
        None => bookmarks
            .list(
                ctx.clone(),
                Freshness::MaybeStale,
                &BookmarkPrefix::empty(),
                BookmarkKind::ALL_PUBLISHING,
                &BookmarkPagination::FromStart,
                u64::MAX,
            )
            .map_ok(|(_bookmark, cs_id)| cs_id)
            .left_stream(),
        Some(bookmark_name) => stream::once(
            bookmarks
                .get(ctx.clone(), bookmark_name)
                .and_then({
                    let bookmark_name = bookmark_name.clone();
                    move |opt_cs_id| async move {
                        opt_cs_id.ok_or_else({
                            move || format_err!("'{}' bookmark could not be found", bookmark_name)
                        })
                    }
                })
                .map({
                    let bookmark_name = bookmark_name.clone();
                    move |r| {
                        r.with_context(|| {
                            format!(
                                "error while fetching changeset for bookmark {}",
                                bookmark_name
                            )
                        })
                    }
                }),
        )
        .right_stream(),
    };
    Ok(VertexListWithOptions::from(
        bm_stream
            .map_ok(|cs| head_with_options(cs))
            .try_collect::<Vec<_>>()
            .await?,
    ))
}
