/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use bookmarks::BookmarkName;
use metaconfig_types::BookmarkAttrs;
use mononoke_types::ChangesetId;

use unbundle::{
    run_post_resolve_action, InfiniteBookmarkPush, PlainBookmarkPush, PostResolveAction,
    PostResolveBookmarkOnlyPushRebase, PostResolveInfinitePush, UploadedBonsais,
    UploadedHgChangesetIds,
};

use crate::errors::MononokeError;
use crate::repo_write::RepoWriteContext;

impl RepoWriteContext {
    /// Move a bookmark.
    pub async fn move_bookmark(
        &self,
        bookmark: impl AsRef<str>,
        target: ChangesetId,
        allow_non_fast_forward: bool,
    ) -> Result<(), MononokeError> {
        let bookmark = bookmark.as_ref();
        self.check_method_permitted("move_bookmark")?;
        self.check_bookmark_modification_permitted(bookmark)?;

        let bookmark = BookmarkName::new(bookmark)?;
        let bookmark_attrs = BookmarkAttrs::new(self.config().bookmarks.clone());

        // Check if this bookmark has hooks associated with it.  We don't support
        // hooks on plain bookmark moves yet.
        bookmark_attrs.select(&bookmark).try_for_each(|params| {
            if params.hooks.is_empty() {
                Ok(())
            } else {
                Err(MononokeError::NotAvailable(format!(
                    "move_bookmark not available for {} because it has hooks",
                    bookmark
                )))
            }
        })?;

        // We need to work out whether or not this is a scratch bookmark so
        // we can call the right code.
        let is_scratch_bookmark = if let Some(namespace) = &self.config().infinitepush.namespace {
            namespace.matches_bookmark(&bookmark)
        } else {
            false
        };

        // We need to find out where the bookmark currently points to in order
        // to move it.  Make sure to bypass any out-of-date caches.
        let old_target = self
            .blob_repo()
            .bookmarks()
            .get(self.ctx().clone(), &bookmark)
            .await?;

        let action = if is_scratch_bookmark {
            let bookmark_push = InfiniteBookmarkPush {
                name: bookmark,
                create: false,
                force: allow_non_fast_forward,
                old: old_target,
                new: target,
            };

            PostResolveAction::InfinitePush(PostResolveInfinitePush {
                changegroup_id: None,
                maybe_bookmark_push: Some(bookmark_push),
                mutations: Vec::new(),
                maybe_raw_bundle2_id: None,
                uploaded_bonsais: UploadedBonsais::new(),
                uploaded_hg_changeset_ids: UploadedHgChangesetIds::new(),
                is_cross_backend_sync: false,
            })
        } else {
            let bookmark_push = PlainBookmarkPush {
                part_id: 0u32, // Just make something up.
                name: bookmark,
                old: old_target,
                new: Some(target),
            };

            PostResolveAction::BookmarkOnlyPushRebase(PostResolveBookmarkOnlyPushRebase {
                bookmark_push,
                maybe_raw_bundle2_id: None,
                non_fast_forward_policy: allow_non_fast_forward.into(),
            })
        };

        let _response = run_post_resolve_action(
            self.ctx(),
            self.blob_repo(),
            &bookmark_attrs,
            self.skiplist_index(),
            &self.config().infinitepush,
            &self.config().pushrebase,
            &self.config().push,
            None, // maybe_reverse_filler_queue
            action,
        )
        .await
        .map_err(anyhow::Error::from)?;

        Ok(())
    }
}
