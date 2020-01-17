/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use crate::CommonHeads;
use anyhow::Error;
use blobrepo::BlobRepo;
use bookmarks::BookmarkName;
use bytes::{Bytes, BytesMut};
use context::CoreContext;
use failure_ext::FutureFailureErrorExt;
use futures::{Future, Stream};
use futures_ext::{try_boxfuture, BoxFuture, FutureExt};
use futures_stats::Timed;
use getbundle_response::create_getbundle_response;
use mercurial_bundles::{create_bundle_stream, parts, Bundle2EncodeBuilder, PartId};
use metaconfig_types::PushrebaseParams;
use mononoke_types::ChangesetId;
use obsolete;
use phases::Phases;
use pushrebase;
use reachabilityindex::LeastCommonAncestorsHint;
use scuba_ext::ScubaSampleBuilderExt;
use std::io::Cursor;
use std::sync::Arc;

/// Data, needed to generate a `Push` response
pub struct UnbundlePushResponse {
    pub changegroup_id: Option<PartId>,
    pub bookmark_ids: Vec<PartId>,
}

/// Data, needed to generate an `InfinitePush` response
pub struct UnbundleInfinitePushResponse {
    pub changegroup_id: Option<PartId>,
}

/// Data, needed to generate a `PushRebase` response
pub struct UnbundlePushRebaseResponse {
    pub commonheads: CommonHeads,
    pub pushrebased_rev: ChangesetId,
    pub pushrebased_changesets: Vec<pushrebase::PushrebaseChangesetPair>,
    pub onto: BookmarkName,
    pub bookmark_push_part_id: Option<PartId>,
}

/// Data, needed to generate a bookmark-only `PushRebase` response
pub struct UnbundleBookmarkOnlyPushRebaseResponse {
    pub bookmark_push_part_id: PartId,
}

pub enum UnbundleResponse {
    Push(UnbundlePushResponse),
    InfinitePush(UnbundleInfinitePushResponse),
    PushRebase(UnbundlePushRebaseResponse),
    BookmarkOnlyPushRebase(UnbundleBookmarkOnlyPushRebaseResponse),
}

impl UnbundleResponse {
    fn get_bundle_builder() -> Bundle2EncodeBuilder<Cursor<Vec<u8>>> {
        let writer = Cursor::new(Vec::new());
        let mut bundle = Bundle2EncodeBuilder::new(writer);
        // Mercurial currently hangs while trying to read compressed bundles over the wire:
        // https://bz.mercurial-scm.org/show_bug.cgi?id=5646
        // TODO: possibly enable compression support once this is fixed.
        bundle.set_compressor_type(None);
        bundle
    }

    fn generate_push_or_infinitepush_response(
        changegroup_id: Option<PartId>,
        bookmark_ids: Vec<PartId>,
    ) -> BoxFuture<Bytes, Error> {
        let mut bundle = Self::get_bundle_builder();
        if let Some(changegroup_id) = changegroup_id {
            bundle.add_part(try_boxfuture!(parts::replychangegroup_part(
                parts::ChangegroupApplyResult::Success { heads_num_diff: 0 },
                changegroup_id,
            )));
        }
        for part_id in bookmark_ids {
            bundle.add_part(try_boxfuture!(parts::replypushkey_part(true, part_id)));
        }
        bundle
            .build()
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .boxify()
    }

    fn generate_push_response_bytes(
        _ctx: CoreContext,
        data: UnbundlePushResponse,
    ) -> BoxFuture<Bytes, Error> {
        let UnbundlePushResponse {
            changegroup_id,
            bookmark_ids,
        } = data;
        Self::generate_push_or_infinitepush_response(changegroup_id, bookmark_ids)
            .context("While preparing push response")
            .from_err()
            .boxify()
    }

    fn generate_inifinitepush_response_bytes(
        _ctx: CoreContext,
        data: UnbundleInfinitePushResponse,
    ) -> BoxFuture<Bytes, Error> {
        let UnbundleInfinitePushResponse { changegroup_id } = data;
        Self::generate_push_or_infinitepush_response(changegroup_id, vec![])
            .context("While preparing infinitepush response")
            .from_err()
            .boxify()
    }

    fn generate_pushrebase_response_bytes(
        ctx: CoreContext,
        data: UnbundlePushRebaseResponse,
        repo: BlobRepo,
        pushrebase_params: PushrebaseParams,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        phases: Arc<dyn Phases>,
    ) -> BoxFuture<Bytes, Error> {
        let UnbundlePushRebaseResponse {
            commonheads,
            pushrebased_rev,
            pushrebased_changesets,
            onto,
            bookmark_push_part_id,
        } = data;

        // Send to the client both pushrebased commit and current "onto" bookmark. Normally they
        // should be the same, however they might be different if bookmark
        // suddenly moved before current pushrebase finished.
        let common = commonheads.heads;
        let maybe_onto_head = repo.get_bookmark(ctx.clone(), &onto);

        let pushrebased_hg_rev = repo.get_hg_from_bonsai_changeset(ctx.clone(), pushrebased_rev);

        let bookmark_reply_part = match bookmark_push_part_id {
            Some(part_id) => Some(try_boxfuture!(parts::replypushkey_part(true, part_id))),
            None => None,
        };

        let obsmarkers_part = match pushrebase_params.emit_obsmarkers {
            true => try_boxfuture!(obsolete::pushrebased_changesets_to_obsmarkers_part(
                ctx.clone(),
                &repo,
                pushrebased_changesets,
            )
            .transpose()),
            false => None,
        };

        let mut scuba_logger = ctx.scuba().clone();
        maybe_onto_head
            .join(pushrebased_hg_rev)
            .and_then(move |(maybe_onto_head, pushrebased_hg_rev)| {
                let mut heads = vec![];
                if let Some(onto_head) = maybe_onto_head {
                    heads.push(onto_head);
                }
                heads.push(pushrebased_hg_rev);
                create_getbundle_response(ctx, repo, common, heads, lca_hint, Some(phases))
            })
            .and_then(move |mut cg_part_builder| {
                cg_part_builder.extend(bookmark_reply_part.into_iter());
                cg_part_builder.extend(obsmarkers_part.into_iter());
                let compression = None;
                create_bundle_stream(cg_part_builder, compression)
                    .collect()
                    .map(|chunks| {
                        let mut total_capacity = 0;
                        for c in chunks.iter() {
                            total_capacity += c.len();
                        }

                        // TODO(stash): make push and pushrebase response streamable - T34090105
                        let mut res = BytesMut::with_capacity(total_capacity);
                        for c in chunks {
                            res.extend_from_slice(&c);
                        }
                        res.freeze()
                    })
            })
            .timed({
                move |stats, result| {
                    if result.is_ok() {
                        scuba_logger
                            .add_future_stats(&stats)
                            .log_with_msg("Pushrebase: prepared the response", None);
                    }
                    Ok(())
                }
            })
            .context("While preparing pushrebase response")
            .from_err()
            .boxify()
    }

    fn generate_bookmark_only_pushrebase_response_bytes(
        _ctx: CoreContext,
        data: UnbundleBookmarkOnlyPushRebaseResponse,
    ) -> BoxFuture<Bytes, Error> {
        let UnbundleBookmarkOnlyPushRebaseResponse {
            bookmark_push_part_id,
        } = data;

        let mut bundle = Self::get_bundle_builder();
        bundle.add_part(try_boxfuture!(parts::replypushkey_part(
            true,
            bookmark_push_part_id
        )));
        bundle
            .build()
            .map(|cursor| Bytes::from(cursor.into_inner()))
            .context("While preparing bookmark-only pushrebase response")
            .from_err()
            .boxify()
    }

    /// Produce bundle2 response parts for the completed `unbundle` processing
    pub fn generate_bytes(
        self,
        ctx: CoreContext,
        repo: BlobRepo,
        pushrebase_params: PushrebaseParams,
        lca_hint: Arc<dyn LeastCommonAncestorsHint>,
        phases: Arc<dyn Phases>,
    ) -> BoxFuture<Bytes, Error> {
        match self {
            UnbundleResponse::Push(data) => Self::generate_push_response_bytes(ctx, data),
            UnbundleResponse::InfinitePush(data) => {
                Self::generate_inifinitepush_response_bytes(ctx, data)
            }
            UnbundleResponse::PushRebase(data) => Self::generate_pushrebase_response_bytes(
                ctx,
                data,
                repo,
                pushrebase_params,
                lca_hint,
                phases,
            ),
            UnbundleResponse::BookmarkOnlyPushRebase(data) => {
                Self::generate_bookmark_only_pushrebase_response_bytes(ctx, data)
            }
        }
    }
}
