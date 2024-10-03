/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::Error;
use blobstore::Loadable;
use bookmarks::ArcBookmarks;
use bookmarks::BookmarkKey;
use context::CoreContext;
use fbinit::FacebookInit;
use fsnodes::RootFsnodeId;
use futures::future::abortable;
use futures::future::AbortHandle;
use metaconfig_types::ArcRepoConfig;
use mononoke_types::ChangesetId;
use repo_blobstore::ArcRepoBlobstore;
use repo_derived_data::ArcRepoDerivedData;
use sharding_ext::encode_repo_name;
use slog::warn;
use slog::Logger;
use stats::define_stats;
use stats::prelude::DynamicSingletonCounter;

define_stats! {
    prefix = "mononoke.app.repo.stats";
    repo_objects_count: dynamic_singleton_counter("{}.objects.count", (repo_name: String)),
}

const DEFAULT_REPO_OBJECTS_COUNT: i64 = 1_000_000;

#[derive(Clone)]
#[facet::facet]
pub struct RepoStatsLogger {
    abort_handle: AbortHandle,
}

impl RepoStatsLogger {
    pub async fn new(
        fb: FacebookInit,
        logger: Logger,
        repo_name: String,
        repo_config: ArcRepoConfig,
        bookmarks: ArcBookmarks,
        repo_blobstore: ArcRepoBlobstore,
        repo_derived_data: ArcRepoDerivedData,
    ) -> Result<Self, Error> {
        let ctx = CoreContext::new_for_bulk_processing(fb, logger.clone());

        let fut = async move {
            loop {
                let interval = Duration::from_secs(60);
                tokio::time::sleep(interval).await;

                let repo_key = encode_repo_name(&repo_name);
                let bookmark_name =
                    get_repo_bookmark_name(repo_config.clone()).expect("invalid bookmark name");

                match get_repo_objects_count(
                    &ctx,
                    &repo_name,
                    &repo_config,
                    &bookmark_name,
                    bookmarks.clone(),
                    repo_blobstore.clone(),
                    repo_derived_data.clone(),
                )
                .await
                {
                    Ok(count) => {
                        STATS::repo_objects_count.set_value(fb, count, (repo_key,));
                    }
                    Err(e) => {
                        warn!(ctx.logger(), "Finding bookmark for {}: {}", repo_name, e);
                    }
                }
            }
        };

        let (fut, abort_handle) = abortable(fut);
        tokio::spawn(fut);

        Ok(Self { abort_handle })
    }

    // A null implementation that does nothing. Useful for tests.
    pub fn noop() -> Self {
        Self {
            abort_handle: AbortHandle::new_pair().0,
        }
    }
}

fn get_repo_bookmark_name(
    repo_config: Arc<metaconfig_types::RepoConfig>,
) -> Result<BookmarkKey, Error> {
    let bookmark_name = repo_config
        .bookmark_name_for_objects_count
        .clone()
        .unwrap_or("master".to_string());
    BookmarkKey::new(bookmark_name)
}

//
// Returns the settings for repo objectos count computations.
//
fn get_repo_objects_count_settings(
    repo_name: &str,
    repo_config: Arc<metaconfig_types::RepoConfig>,
) -> (i64, Option<i64>) {
    let default_objects_count = repo_config
        .default_objects_count
        .clone()
        .or_else(|| justknobs::get("scm/mononoke:scs_default_repo_objects_count", None).ok())
        .unwrap_or(DEFAULT_REPO_OBJECTS_COUNT);

    let maybe_override_objects_count = justknobs::get_as::<i64>(
        "scm/mononoke:scs_override_repo_objects_count",
        Some(repo_name),
    )
    .ok()
    .or_else(|| repo_config.override_objects_count.clone());

    (default_objects_count, maybe_override_objects_count)
}

async fn get_repo_objects_count(
    ctx: &CoreContext,
    repo_name: &str,
    repo_config: &ArcRepoConfig,
    bookmark_name: &BookmarkKey,
    bookmarks: ArcBookmarks,
    repo_blobstore: ArcRepoBlobstore,
    repo_derived_data: ArcRepoDerivedData,
) -> Result<i64, Error> {
    let (default_repo_objects_count, maybe_override_objects_count) =
        get_repo_objects_count_settings(repo_name, repo_config.clone());

    match maybe_override_objects_count {
        Some(over) => {
            // whether the override comes from config or JK, we will skip any computation
            Ok(over)
        }
        None => {
            let maybe_bookmark = bookmarks.get(ctx.clone(), bookmark_name).await?;
            if let Some(cs_id) = maybe_bookmark {
                get_descendant_count(
                    ctx,
                    repo_blobstore.clone(),
                    repo_derived_data.clone(),
                    cs_id,
                )
                .await
            } else {
                Ok(default_repo_objects_count)
            }
        }
    }
}

async fn get_descendant_count(
    ctx: &CoreContext,
    repo_blobstore: ArcRepoBlobstore,
    repo_derived_data: ArcRepoDerivedData,
    cs_id: ChangesetId,
) -> Result<i64, Error> {
    let root_fsnode_id = repo_derived_data.derive::<RootFsnodeId>(ctx, cs_id).await?;
    let count = root_fsnode_id
        .fsnode_id()
        .load(ctx, &repo_blobstore)
        .await?
        .summary()
        .descendant_files_count;
    Ok(i64::try_from(count).expect("file count overflows i64"))
}

impl Drop for RepoStatsLogger {
    fn drop(&mut self) {
        self.abort_handle.abort()
    }
}

#[cfg(test)]
mod tests {
    use bookmarks::Bookmarks;
    use bookmarks::BookmarksArc;
    use fbinit::FacebookInit;
    use futures::future::FutureExt;
    use justknobs::test_helpers::with_just_knobs;
    use justknobs::test_helpers::with_just_knobs_async;
    use justknobs::test_helpers::JustKnobsInMemory;
    use justknobs::test_helpers::KnobVal;
    use maplit::hashmap;
    use metaconfig_types::RepoConfig;
    use metaconfig_types::RepoConfigArc;
    use mononoke_macros::mononoke;
    use repo_blobstore::RepoBlobstore;
    use repo_blobstore::RepoBlobstoreArc;
    use repo_derived_data::RepoDerivedData;
    use repo_derived_data::RepoDerivedDataArc;
    use test_repo_factory::TestRepoFactory;

    use super::*;

    #[facet::container]
    #[derive(Clone)]
    struct Repo {
        #[facet]
        repo_config: RepoConfig,

        #[facet]
        bookmarks: dyn Bookmarks,

        #[facet]
        repo_blobstore: RepoBlobstore,

        #[facet]
        derived_data: RepoDerivedData,
    }

    #[mononoke::fbinit_test]
    async fn test_get_repo_objects_count_settings(fb: FacebookInit) -> Result<(), Error> {
        let factory = TestRepoFactory::new(fb)?;
        let repo: Repo = factory.build().await?;

        let (default_objects_count, maybe_override_objects_count) =
            get_repo_objects_count_settings("repo", repo.repo_config_arc());
        assert_eq!(default_objects_count, 1000000);
        assert!(maybe_override_objects_count.is_none());

        // set via JK
        let (default_objects_count, maybe_override_objects_count) = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:scs_default_repo_objects_count".to_string() => KnobVal::Int(10),
                "scm/mononoke:scs_override_repo_objects_count".to_string() => KnobVal::Int(20),
            ]),
            || get_repo_objects_count_settings("repo", repo.repo_config_arc()),
        );
        assert_eq!(default_objects_count, 10);
        assert_eq!(maybe_override_objects_count.unwrap(), 20);

        // set in the repo config
        let repo_config = Arc::new(RepoConfig {
            default_objects_count: Some(100),
            override_objects_count: Some(200),
            ..Default::default()
        });
        let (default_objects_count, maybe_override_objects_count) =
            get_repo_objects_count_settings("repo", repo_config);
        assert_eq!(default_objects_count, 100);
        assert_eq!(maybe_override_objects_count.unwrap(), 200);

        // set in both the repo config and in the JK
        let repo_config = Arc::new(RepoConfig {
            default_objects_count: Some(1000),
            override_objects_count: Some(4000),
            ..Default::default()
        });
        let (default_objects_count, maybe_override_objects_count) = with_just_knobs(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:scs_default_repo_objects_count".to_string() => KnobVal::Int(2000),
                "scm/mononoke:scs_override_repo_objects_count".to_string() => KnobVal::Int(3000),
            ]),
            || get_repo_objects_count_settings("repo", repo_config),
        );
        assert_eq!(default_objects_count, 1000);
        assert_eq!(maybe_override_objects_count.unwrap(), 3000);

        Ok(())
    }

    #[mononoke::fbinit_test]
    async fn test_get_repo_objects_count(fb: FacebookInit) -> Result<(), Error> {
        let ctx = CoreContext::test_mock(fb);
        let factory = TestRepoFactory::new(fb)?;
        let repo: Repo = factory.build().await?;

        let repo_config = repo.repo_config_arc();
        let bookmarks = repo.bookmarks_arc();
        let repo_blobstore = repo.repo_blobstore_arc();
        let repo_derived_data = repo.repo_derived_data_arc();

        let bookmark_key = BookmarkKey::new("master")?;
        let get_count = get_repo_objects_count(
            &ctx,
            "repo",
            &repo_config,
            &bookmark_key,
            bookmarks.clone(),
            repo_blobstore.clone(),
            repo_derived_data.clone(),
        );

        // plain defaults, including the default JK value of 1000000
        let count = get_count.await?;
        assert_eq!(count, 1000000);

        // set a default via JK
        let get_count = get_repo_objects_count(
            &ctx,
            "repo",
            &repo_config,
            &bookmark_key,
            bookmarks.clone(),
            repo_blobstore.clone(),
            repo_derived_data.clone(),
        );
        let count = with_just_knobs_async(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:scs_default_repo_objects_count".to_string() => KnobVal::Int(42)
            ]),
            get_count.boxed(),
        )
        .await?;
        assert_eq!(count, 42);

        // override via JK
        let get_count = get_repo_objects_count(
            &ctx,
            "repo",
            &repo_config,
            &bookmark_key,
            bookmarks.clone(),
            repo_blobstore.clone(),
            repo_derived_data.clone(),
        );
        let count = with_just_knobs_async(
            JustKnobsInMemory::new(hashmap![
                "scm/mononoke:scs_default_repo_objects_count".to_string() => KnobVal::Int(42),
                "scm/mononoke:scs_override_repo_objects_count".to_string() => KnobVal::Int(15),
            ]),
            get_count.boxed(),
        )
        .await?;
        assert_eq!(count, 15);

        Ok(())
    }
}
