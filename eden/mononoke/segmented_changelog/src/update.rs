/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use futures::future;
use futures::stream;
use futures::stream::TryStreamExt;
use slog::info;
use std::collections::HashSet;

use bookmarks::BookmarkKind;
use bookmarks::BookmarkName;
use bookmarks::BookmarkPagination;
use bookmarks::BookmarkPrefix;
use bookmarks::Bookmarks;
use bookmarks::Freshness;
use context::CoreContext;
use metaconfig_types::SegmentedChangelogConfig;
use metaconfig_types::SegmentedChangelogHeadConfig;
use mononoke_types::ChangesetId;

use crate::dag::NameDagBuilder;
use crate::dag::VertexListWithOptions;
use crate::dag::VertexName;
use crate::dag::VertexOptions;
use crate::idmap::vertex_name_from_cs_id;
use crate::idmap::IdMap;
use crate::idmap::IdMapWrapper;
use crate::Group;
use crate::InProcessIdDag;

pub type SeedHead = SegmentedChangelogHeadConfig;

#[async_trait::async_trait]
trait IntoVertexList {
    async fn into_vertex_list(
        &self,
        ctx: &CoreContext,
        bookmarks: &dyn Bookmarks,
    ) -> Result<VertexListWithOptions>;
}

#[async_trait::async_trait]
impl IntoVertexList for SeedHead {
    async fn into_vertex_list(
        &self,
        ctx: &CoreContext,
        bookmarks: &dyn Bookmarks,
    ) -> Result<VertexListWithOptions> {
        match self {
            Self::Changeset(id) => Ok(VertexListWithOptions::from(vec![head_with_options(id)])),
            Self::AllPublicBookmarksExcept(exceptions) => {
                all_bookmarks_except_with_options(ctx, exceptions, bookmarks).await
            }
            Self::Bookmark(name) => bookmark_with_options(ctx, name, bookmarks).await,
        }
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum JobType {
    Background,
    Server,
}

pub fn seedheads_from_config(
    ctx: &CoreContext,
    config: &SegmentedChangelogConfig,
    job_type: JobType,
) -> Result<Vec<SeedHead>> {
    let mut heads = config.heads_to_include.clone();

    if job_type == JobType::Background {
        heads.extend(config.extra_heads_to_include_in_background_jobs.clone());
    }

    info!(
        ctx.logger(),
        "Using the following segmented changelog heads: {:?}", heads
    );
    Ok(heads)
}

pub async fn vertexlist_from_seedheads(
    ctx: &CoreContext,
    heads: &[SeedHead],
    bookmarks: &dyn Bookmarks,
) -> Result<VertexListWithOptions> {
    let heads_with_options = stream::iter(heads.iter().map(Result::Ok))
        .try_fold(VertexListWithOptions::default(), {
            move |acc, head| async move {
                Ok::<_, Error>(acc.chain(head.into_vertex_list(ctx, bookmarks).await?))
            }
        })
        .await?;

    Ok(heads_with_options)
}

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

fn head_with_options(head: &ChangesetId) -> (VertexName, VertexOptions) {
    let mut options = VertexOptions::default();
    options.reserve_size = 1 << 26;
    options.highest_group = Group::MASTER;
    (vertex_name_from_cs_id(head), options)
}

async fn all_bookmarks_except_with_options(
    ctx: &CoreContext,
    exceptions: &[BookmarkName],
    bookmarks: &dyn Bookmarks,
) -> Result<VertexListWithOptions> {
    let exceptions: HashSet<_> = exceptions.iter().cloned().collect();
    Ok(bookmarks
        .list(
            ctx.clone(),
            Freshness::MaybeStale,
            &BookmarkPrefix::empty(),
            BookmarkKind::ALL_PUBLISHING,
            &BookmarkPagination::FromStart,
            u64::MAX,
        )
        .try_filter_map(|(bookmark, cs_id)| {
            let res = if exceptions.contains(bookmark.name()) {
                None
            } else {
                Some(cs_id)
            };
            future::ready(Ok(res))
        })
        .map_ok(|cs| head_with_options(&cs))
        .try_collect::<Vec<_>>()
        .await?
        .into())
}

async fn bookmark_with_options(
    ctx: &CoreContext,
    bookmark: &BookmarkName,
    bookmarks: &dyn Bookmarks,
) -> Result<VertexListWithOptions> {
    let cs = bookmarks
        .get(ctx.clone(), bookmark)
        .await
        .with_context(|| format!("error while fetching changeset for bookmark {}", bookmark))?
        .ok_or_else(move || format_err!("'{}' bookmark could not be found", bookmark))?;
    Ok(VertexListWithOptions::from(vec![head_with_options(&cs)]))
}

#[cfg(test)]
mod tests {
    use super::*;
    use blobrepo::BlobRepo;
    use fbinit::FacebookInit;
    use fixtures::set_bookmark;
    use fixtures::BranchWide;
    use fixtures::TestRepoFixture;

    async fn prep_branch_wide_repo(fb: FacebookInit) -> Result<Arc<BlobRepo>> {
        let blobrepo = BranchWide::getrepo(fb).await;
        let second = BookmarkName::new("second")?;
        set_bookmark(
            fb,
            &blobrepo,
            "04decbb0d1a65789728250ddea2fe8d00248e01c",
            second,
        )
        .await;
        let third = BookmarkName::new("third")?;
        set_bookmark(
            fb,
            &blobrepo,
            "c27ef5b7f15e9930e5b93b1f32cc2108a2aabe12",
            third,
        )
        .await;
        Ok(Arc::new(blobrepo))
    }

    #[fbinit::test]
    async fn test_bookmark_with_options(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = prep_branch_wide_repo(fb).await?;
        let second = BookmarkName::new("second")?;

        let res = bookmark_with_options(&ctx, &second, repo.bookmarks().as_ref()).await?;
        assert_eq!(
            res.vertexes(),
            vec![VertexName::from_hex(
                b"5ec506306edb84a4a47f901a55cedeec3113eb118bfae119982f45382481e3dc"
            )?,]
        );
        Ok(())
    }

    #[fbinit::test]
    async fn test_all_bookmarks_except_with_options(fb: FacebookInit) -> Result<()> {
        let ctx = CoreContext::test_mock(fb);
        let repo = prep_branch_wide_repo(fb).await?;

        let res =
            all_bookmarks_except_with_options(&ctx, vec![].as_slice(), repo.bookmarks().as_ref())
                .await?;
        assert_eq!(
            res.vertexes(),
            vec![
                VertexName::from_hex(
                    b"56da5b997e27f2f9020f6ff2d87b321774369e23579bd2c4ce675efad363f4f4"
                )?,
                VertexName::from_hex(
                    b"5ec506306edb84a4a47f901a55cedeec3113eb118bfae119982f45382481e3dc"
                )?,
                VertexName::from_hex(
                    b"7097e8d1e72af16e8135047d8693fb381246be1bc74c1b6c0cb013fc05331fc1"
                )?,
            ]
        );

        let res = all_bookmarks_except_with_options(
            &ctx,
            vec![BookmarkName::new("second")?].as_slice(),
            repo.bookmarks().as_ref(),
        )
        .await?;
        assert_eq!(
            res.vertexes(),
            vec![
                VertexName::from_hex(
                    b"56da5b997e27f2f9020f6ff2d87b321774369e23579bd2c4ce675efad363f4f4"
                )?,
                VertexName::from_hex(
                    b"7097e8d1e72af16e8135047d8693fb381246be1bc74c1b6c0cb013fc05331fc1"
                )?,
            ]
        );
        Ok(())
    }
}
