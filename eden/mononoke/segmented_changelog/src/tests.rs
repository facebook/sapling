/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::sync::Arc;
use std::time::Duration;

use anyhow::format_err;
use anyhow::Context;
use anyhow::Result;
use fbinit::FacebookInit;
use futures::compat::Stream01CompatExt;
use futures::future::try_join_all;
use futures::future::FutureExt;
use futures::stream;
use futures::StreamExt;
use once_cell::sync::Lazy;

use blobrepo::AsBlobRepo;
use blobrepo::BlobRepo;
use bonsai_hg_mapping::BonsaiHgMappingArc;
use bookmarks::BookmarkName;
use bookmarks::Bookmarks;
use bookmarks::BookmarksArc;
use bulkops::PublicChangesetBulkFetch;
use caching_ext::CachelibHandler;
use caching_ext::MemcacheHandler;
use changeset_fetcher::PrefetchedChangesetsFetcher;
use changesets::ChangesetEntry;
use changesets::ChangesetsArc;
use changesets::ChangesetsRef;
use context::CoreContext;
use fixtures::set_bookmark;
use fixtures::BranchEven;
use fixtures::Linear;
use fixtures::MergeEven;
use fixtures::MergeUneven;
use fixtures::TestRepoFixture;
use fixtures::UnsharedMergeEven;
use maplit::hashmap;
use mononoke_types::ChangesetId;
use mononoke_types::RepositoryId;
use phases::PhasesArc;
use phases::PhasesRef;
use revset::AncestorsNodeStream;
use sql_construct::SqlConstruct;
use sql_ext::replication::NoReplicaLagMonitor;
use tests_utils::resolve_cs_id;
use tests_utils::CreateCommitContext;
use tunables::override_tunables;
use tunables::with_tunables_async;

use crate::builder::SegmentedChangelogSqlConnections;
use crate::iddag::IdDagSaveStore;
use crate::idmap::CacheHandlers;
use crate::idmap::ConcurrentMemIdMap;
use crate::idmap::IdMap;
use crate::idmap::IdMapFactory;
use crate::idmap::SqlIdMap;
use crate::manager::SegmentedChangelogManager;
use crate::manager::SegmentedChangelogType;
use crate::on_demand::OnDemandUpdateSegmentedChangelog;
use crate::owned::OwnedSegmentedChangelog;
use crate::periodic_reload::PeriodicReloadSegmentedChangelog;
use crate::tailer::SegmentedChangelogTailer;
use crate::types::IdDagVersion;
use crate::types::IdMapVersion;
use crate::types::SegmentedChangelogVersion;
use crate::version_store::SegmentedChangelogVersionStore;
use crate::CloneHints;
use crate::InProcessIdDag;
use crate::Location;
use crate::SeedHead;
use crate::SegmentedChangelog;
use crate::SegmentedChangelogRef;

#[async_trait::async_trait]
trait SegmentedChangelogExt {
    async fn head(&self, ctx: &CoreContext) -> Result<ChangesetId>;
}

#[async_trait::async_trait]
impl<T> SegmentedChangelogExt for T
where
    T: SegmentedChangelog,
{
    async fn head(&self, ctx: &CoreContext) -> Result<ChangesetId> {
        let clone_data = self.clone_data(ctx).await?;
        let cs_id = clone_data
            .0
            .idmap
            .iter()
            .max_by_key(|i| i.0)
            .map(|i| i.1.clone())
            .ok_or_else(|| format_err!("error with clone data, missing idmap entry for head_id"))?;
        Ok(cs_id)
    }
}

static BOOKMARK_NAME: Lazy<BookmarkName> = Lazy::new(|| BookmarkName::new("master").unwrap());

async fn new_tailer_for_tailing(
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
) -> Result<SegmentedChangelogTailer> {
    new_tailer(blobrepo, connections, None, None).await
}

async fn new_tailer(
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
    prefetched: Option<Vec<ChangesetEntry>>,
    seed_heads: Option<Vec<ChangesetId>>,
) -> Result<SegmentedChangelogTailer> {
    let prefetched = match prefetched {
        None => stream::empty().boxed(),
        Some(prefetched) => stream::iter(prefetched.into_iter().map(Ok)).boxed(),
    };

    let changeset_fetcher = Arc::new(
        PrefetchedChangesetsFetcher::new(
            blobrepo.get_repoid(),
            blobrepo.changesets_arc(),
            prefetched,
        )
        .await?,
    );

    let bulk_fetcher = Arc::new(PublicChangesetBulkFetch::new(
        blobrepo.changesets_arc(),
        blobrepo.phases_arc(),
    ));

    let seed_heads = match seed_heads {
        Some(heads) => heads.into_iter().map(SeedHead::from).collect(),
        None => vec![Some(BOOKMARK_NAME.clone()).into()],
    };

    Ok(SegmentedChangelogTailer::new(
        blobrepo.get_repoid(),
        connections.clone(),
        Arc::new(NoReplicaLagMonitor()),
        changeset_fetcher,
        bulk_fetcher,
        blobrepo.bonsai_hg_mapping_arc(),
        Arc::new(blobrepo.get_blobstore()),
        blobrepo.bookmarks_arc(),
        seed_heads,
        None,
    ))
}

async fn seed_with_prefetched(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
    heads: Vec<ChangesetId>,
    prefetched: Option<Vec<ChangesetEntry>>,
) -> Result<()> {
    // Does it make sense to seed up to the master commit instead of head?
    // Something to consider. Depends how it's used by tests.
    blobrepo
        .phases()
        .add_reachable_as_public(ctx, heads.clone())
        .await?;
    let tailer = new_tailer(blobrepo, connections, prefetched, Some(heads)).await?;
    tailer.once(ctx, true).await?;
    Ok(())
}

async fn seed(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
    head: ChangesetId,
) -> Result<()> {
    seed_with_prefetched(
        ctx,
        blobrepo,
        connections,
        vec![head],
        None, // no prefetching
    )
    .await
}

async fn load_sc_version(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    connections: &SegmentedChangelogSqlConnections,
) -> Result<SegmentedChangelogVersion> {
    let sc_version_store = SegmentedChangelogVersionStore::new(connections.0.clone(), repo_id);
    let sc_version = sc_version_store
        .get(ctx)
        .await?
        .context("failed to load segmented changelog version")?;
    Ok(sc_version)
}

async fn load_idmap(
    ctx: &CoreContext,
    repo_id: RepositoryId,
    connections: &SegmentedChangelogSqlConnections,
) -> Result<Arc<dyn IdMap>> {
    let sc_version = load_sc_version(ctx, repo_id, connections).await?;
    let idmap = SqlIdMap::new(
        connections.0.clone(),
        Arc::new(NoReplicaLagMonitor()),
        repo_id,
        sc_version.idmap_version,
    );
    Ok(Arc::new(idmap))
}

async fn load_iddag(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
) -> Result<InProcessIdDag> {
    let sc_version = load_sc_version(ctx, blobrepo.get_repoid(), connections).await?;
    let iddag_save_store =
        IdDagSaveStore::new(blobrepo.get_repoid(), Arc::new(blobrepo.get_blobstore()));
    let iddag = iddag_save_store.load(ctx, sc_version.iddag_version).await?;
    Ok(iddag)
}

async fn load_owned(
    ctx: &CoreContext,
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
) -> Result<OwnedSegmentedChangelog> {
    let iddag = load_iddag(ctx, blobrepo, connections).await?;
    let idmap = load_idmap(ctx, blobrepo.get_repoid(), connections).await?;
    Ok(OwnedSegmentedChangelog::new(iddag, idmap))
}

async fn get_manager(
    blobrepo: &BlobRepo,
    connections: &SegmentedChangelogSqlConnections,
    seed_heads: Vec<SeedHead>,
    segmented_changelog_type: SegmentedChangelogType,
) -> Result<SegmentedChangelogManager> {
    let repo_id = blobrepo.get_repoid();
    let blobstore = Arc::new(blobrepo.get_blobstore());
    let sc_version_store = SegmentedChangelogVersionStore::new(connections.0.clone(), repo_id);
    let iddag_save_store = IdDagSaveStore::new(repo_id, blobstore.clone());
    let clone_hints = CloneHints::new(connections.0.clone(), repo_id, blobstore);
    let idmap_factory = IdMapFactory::new(
        connections.0.clone(),
        Arc::new(NoReplicaLagMonitor()),
        repo_id,
    );
    let manager = SegmentedChangelogManager::new(
        repo_id,
        sc_version_store,
        iddag_save_store,
        idmap_factory,
        blobrepo.get_changeset_fetcher(),
        Arc::clone(blobrepo.bookmarks()) as Arc<dyn Bookmarks>,
        seed_heads,
        segmented_changelog_type,
        Some(clone_hints),
    );
    Ok(manager)
}

async fn validate_build_idmap(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    head: &'static str,
) -> Result<()> {
    let head = resolve_cs_id(&ctx, &blobrepo, head).await?;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    seed(&ctx, &blobrepo, &conns, head).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let mut ancestors =
        AncestorsNodeStream::new(ctx.clone(), &blobrepo.get_changeset_fetcher(), head).compat();
    while let Some(cs_id) = ancestors.next().await {
        let cs_id = cs_id?;
        let parents = blobrepo
            .get_changeset_parents_by_bonsai(ctx.clone(), cs_id)
            .await?;
        for parent in parents {
            let parent_dag_id = sc.idmap.get_dag_id(&ctx, parent).await?;
            let dag_id = sc.idmap.get_dag_id(&ctx, cs_id).await?;
            assert!(parent_dag_id < dag_id);
        }
    }
    Ok(())
}

async fn fetch_cs_entry(ctx: &CoreContext, repo: &BlobRepo, cs: &str) -> Result<ChangesetEntry> {
    let cs = resolve_cs_id(ctx, &repo, cs).await?;
    repo.changesets()
        .get(ctx.clone(), cs)
        .await?
        .ok_or_else(|| format_err!("can't find changeset entry for {}", cs))
}

#[fbinit::test]
async fn test_iddag_save_store(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    let repo_id = blobrepo.get_repoid();

    let known_cs =
        resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;
    seed(&ctx, &blobrepo, &conns, known_cs).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let distance: u64 = 2;
    let answer = sc
        .location_to_changeset_id(&ctx, Location::new(known_cs, distance))
        .await?;
    let expected_cs =
        resolve_cs_id(&ctx, &blobrepo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
    assert_eq!(answer, expected_cs);

    let iddag_save_store = IdDagSaveStore::new(repo_id, Arc::new(blobrepo.get_blobstore()));
    let iddag_version = iddag_save_store.save(&ctx, &sc.iddag).await?;

    assert!(
        iddag_save_store
            .find(&ctx, IdDagVersion::from_serialized_bytes(b"random"))
            .await?
            .is_none()
    );
    let loaded_id_dag = iddag_save_store.load(&ctx, iddag_version).await?;
    let from_save = OwnedSegmentedChangelog::new(loaded_id_dag, sc.idmap.clone());
    let answer = from_save
        .location_to_changeset_id(&ctx, Location::new(known_cs, distance))
        .await?;
    assert_eq!(answer, expected_cs);

    Ok(())
}

#[fbinit::test]
async fn test_build_idmap(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    validate_build_idmap(
        ctx.clone(),
        Linear::getrepo(fb).await,
        "79a13814c5ce7330173ec04d279bf95ab3f652fb",
    )
    .await?;
    validate_build_idmap(
        ctx.clone(),
        MergeEven::getrepo(fb).await,
        "1f6bc010883e397abeca773192f3370558ee1320",
    )
    .await?;
    validate_build_idmap(
        ctx.clone(),
        MergeUneven::getrepo(fb).await,
        "d35b1875cdd1ed2c687e86f1604b9d7e989450cb",
    )
    .await?;
    Ok(())
}

#[fbinit::test]
async fn test_is_ancestor(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let repo = MergeUneven::get_inner_repo(fb).await;
    let sc = repo.segmented_changelog();
    let blobrepo = repo.as_blob_repo();

    // Graph looks like:
    // a -> b -> c
    //  \-> d -> e
    let a = resolve_cs_id(&ctx, &blobrepo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await?;
    let b = resolve_cs_id(&ctx, &blobrepo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
    let c = resolve_cs_id(&ctx, &blobrepo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await?;
    let d = resolve_cs_id(&ctx, &blobrepo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await?;
    let e = resolve_cs_id(&ctx, &blobrepo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await?;

    assert_eq!(sc.is_ancestor(&ctx, a, c).await?, None);
    assert!(sc.build_up_to_heads(&ctx, &[c, e]).await?);
    assert_eq!(sc.is_ancestor(&ctx, a, c).await?, Some(true));
    assert_eq!(sc.is_ancestor(&ctx, b, d).await?, Some(false));
    assert_eq!(sc.is_ancestor(&ctx, b, a).await?, Some(false));
    assert_eq!(sc.is_ancestor(&ctx, d, e).await?, Some(true));
    assert_eq!(sc.is_ancestor(&ctx, a, a).await?, Some(true));

    Ok(())
}

async fn validate_location_to_changeset_ids(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    known: &'static str,
    distance_count: (u64, u64),
    expected: Vec<&'static str>,
) -> Result<()> {
    let (distance, count) = distance_count;
    let known_cs_id = resolve_cs_id(&ctx, &blobrepo, known).await?;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    seed(&ctx, &blobrepo, &conns, known_cs_id).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let answer = sc
        .location_to_many_changeset_ids(&ctx, Location::new(known_cs_id, distance), count)
        .await?;
    let expected_cs_ids = try_join_all(
        expected
            .into_iter()
            .map(|id| resolve_cs_id(&ctx, &blobrepo, id)),
    )
    .await?;
    assert_eq!(answer, expected_cs_ids);

    Ok(())
}

#[fbinit::test]
async fn test_location_to_changeset_ids(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    validate_location_to_changeset_ids(
        ctx.clone(),
        Linear::getrepo(fb).await,
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // master commit, message: modified 10
        (4, 3),
        vec![
            "0ed509bf086fadcb8a8a5384dc3b550729b0fc17", // message: added 7
            "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b", // message: added 6
            "cb15ca4a43a59acff5388cea9648c162afde8372", // message: added 5
        ],
    )
    .await?;
    validate_location_to_changeset_ids(
        ctx.clone(),
        MergeUneven::getrepo(fb).await,
        "264f01429683b3dd8042cb3979e8bf37007118bc", // top of merged branch, message: Add 5
        (4, 2),
        vec![
            "795b8133cf375f6d68d27c6c23db24cd5d0cd00f", // message: Add 1
            "4f7f3fd428bec1a48f9314414b063c706d9c1aed", // bottom of branch
        ],
    )
    .await?;
    validate_location_to_changeset_ids(
        ctx.clone(),
        UnsharedMergeEven::getrepo(fb).await,
        "7fe9947f101acb4acf7d945e69f0d6ce76a81113", // master commit
        (1, 1),
        vec!["d592490c4386cdb3373dd93af04d563de199b2fb"], // parent of master, merge commit
    )
    .await?;
    Ok(())
}

#[fbinit::test]
async fn test_location_to_changeset_id_invalid_req(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = UnsharedMergeEven::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    let known_cs_id =
        resolve_cs_id(&ctx, &blobrepo, "7fe9947f101acb4acf7d945e69f0d6ce76a81113").await?;
    seed(&ctx, &blobrepo, &conns, known_cs_id).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;
    assert!(
        sc.location_to_many_changeset_ids(&ctx, Location::new(known_cs_id, 1u64), 2u64)
            .await
            .is_err()
    );
    // TODO(T74320664): Ideally LocationToHash should error when asked to go over merge commit.
    // The parents order is not well defined enough for this not to be ambiguous.
    assert!(
        sc.location_to_many_changeset_ids(&ctx, Location::new(known_cs_id, 2u64), 1u64)
            .await
            .is_ok()
    );
    let second_commit =
        resolve_cs_id(&ctx, &blobrepo, "1700524113b1a3b1806560341009684b4378660b").await?;
    assert!(
        sc.location_to_many_changeset_ids(&ctx, Location::new(second_commit, 1u64), 2u64)
            .await
            .is_err()
    );
    assert!(
        sc.location_to_many_changeset_ids(&ctx, Location::new(second_commit, 2u64), 1u64)
            .await
            .is_err()
    );
    Ok(())
}

async fn validate_changeset_id_to_location(
    ctx: CoreContext,
    blobrepo: BlobRepo,
    server_master: &'static str,
    head: &'static str,
    hg_id: &'static str,
    expected: Option<Location<&'static str>>,
) -> Result<()> {
    let server_master = resolve_cs_id(&ctx, &blobrepo, server_master).await?;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    seed(&ctx, &blobrepo, &conns, server_master).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let head_cs = resolve_cs_id(&ctx, &blobrepo, head).await?;
    let cs_id = resolve_cs_id(&ctx, &blobrepo, hg_id).await?;

    let expected_location = match expected {
        None => None,
        Some(location) => {
            let cs_location = location
                .and_then_descendant(|d| resolve_cs_id(&ctx, &blobrepo, d))
                .await?;
            Some(cs_location)
        }
    };

    let answer = sc
        .changeset_id_to_location(&ctx, vec![head_cs], cs_id)
        .await?;
    assert_eq!(answer, expected_location);

    Ok(())
}

#[fbinit::test]
async fn test_changeset_id_to_location(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    validate_changeset_id_to_location(
        ctx.clone(),
        Linear::getrepo(fb).await,
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // master commit, message: modified 10
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // client head == master_commit
        "0ed509bf086fadcb8a8a5384dc3b550729b0fc17", // message: added 7
        Some(Location::new("79a13814c5ce7330173ec04d279bf95ab3f652fb", 4)),
    )
    .await?;
    validate_changeset_id_to_location(
        ctx.clone(),
        Linear::getrepo(fb).await,
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // master commit, message: modified 10
        "3c15267ebf11807f3d772eb891272b911ec68759", // message: added 9
        "0ed509bf086fadcb8a8a5384dc3b550729b0fc17", // message: added 7
        Some(Location::new("3c15267ebf11807f3d772eb891272b911ec68759", 2)),
    )
    .await?;
    validate_changeset_id_to_location(
        ctx.clone(),
        Linear::getrepo(fb).await,
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // master commit, message: modified 10
        "3c15267ebf11807f3d772eb891272b911ec68759", // message: added 9
        "79a13814c5ce7330173ec04d279bf95ab3f652fb", // our master, client doesn't know
        None,
    )
    .await?;
    validate_changeset_id_to_location(
        ctx.clone(),
        MergeUneven::getrepo(fb).await,
        "d35b1875cdd1ed2c687e86f1604b9d7e989450cb", // master, message: Merge two branches
        "d35b1875cdd1ed2c687e86f1604b9d7e989450cb", // client head == master
        "fc2cef43395ff3a7b28159007f63d6529d2f41ca", // message: Add 4
        Some(Location::new("264f01429683b3dd8042cb3979e8bf37007118bc", 2)), // message: add 5
    )
    .await?;
    validate_changeset_id_to_location(
        ctx.clone(),
        UnsharedMergeEven::getrepo(fb).await,
        "7fe9947f101acb4acf7d945e69f0d6ce76a81113", // master commit
        "33fb49d8a47b29290f5163e30b294339c89505a2", // head merged branch, message: Add 5
        "163adc0d0f5d2eb0695ca123addcb92bab202096", // message: Add 3
        Some(Location::new("33fb49d8a47b29290f5163e30b294339c89505a2", 2)),
    )
    .await?;
    Ok(())
}

#[fbinit::test]
async fn test_changeset_id_to_location_random_hash(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let server_master =
        resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    seed(&ctx, &blobrepo, &conns, server_master).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let head_cs = server_master;
    let random_cs_id = mononoke_types_mocks::changesetid::ONES_CSID;

    let answer = sc
        .changeset_id_to_location(&ctx, vec![head_cs], random_cs_id)
        .await?;
    assert_eq!(answer, None);

    Ok(())
}

#[fbinit::test]
async fn test_changeset_id_to_location_multiple_heads(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = BranchEven::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    // Graph looks like:
    // a -> b -> c
    //  \-> d -> e

    let a = resolve_cs_id(&ctx, &blobrepo, "15c40d0abc36d47fb51c8eaec51ac7aad31f669c").await?;
    let b = resolve_cs_id(&ctx, &blobrepo, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
    let c = resolve_cs_id(&ctx, &blobrepo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await?;
    let d = resolve_cs_id(&ctx, &blobrepo, "3cda5c78aa35f0f5b09780d971197b51cad4613a").await?;
    let e = resolve_cs_id(&ctx, &blobrepo, "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5").await?;

    seed(&ctx, &blobrepo, &conns, c).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], a).await?,
        Some(Location::new(c, 2))
    );
    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], d).await?,
        None
    );

    set_bookmark(
        fb,
        &blobrepo,
        "1d8a907f7b4bf50c6a09c16361e2205047ecc5e5",
        BOOKMARK_NAME.clone(),
    )
    .await;
    let tailer = new_tailer_for_tailing(&blobrepo, &conns).await?;
    let _ = tailer.once(&ctx, false).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], b).await?,
        Some(Location::new(c, 1))
    );
    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], d).await?,
        Some(Location::new(e, 1))
    );
    let ambigous = sc
        .changeset_id_to_location(&ctx, vec![c, e], a)
        .await?
        .unwrap();
    assert!(ambigous == Location::new(c, 2) || ambigous == Location::new(e, 2));

    seed(&ctx, &blobrepo, &conns, e).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], a).await?,
        Some(Location::new(e, 2)),
    );
    assert_eq!(
        sc.changeset_id_to_location(&ctx, vec![c, e], b).await?,
        None
    );

    Ok(())
}

#[fbinit::test]
async fn test_build_incremental_from_scratch(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);

    {
        // linear
        let inner = Linear::get_inner_repo(fb).await;
        let blobrepo = inner.as_blob_repo();
        let sc = inner.segmented_changelog();

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
        let distance: u64 = 4;
        let answer = sc
            .location_to_changeset_id(&ctx, Location::new(known_cs, distance))
            .await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
        assert_eq!(answer, expected_cs);
    }
    {
        // merge_uneven
        let inner = MergeUneven::get_inner_repo(fb).await;
        let blobrepo = inner.as_blob_repo();
        let sc = inner.segmented_changelog();

        let known_cs =
            resolve_cs_id(&ctx, &blobrepo, "264f01429683b3dd8042cb3979e8bf37007118bc").await?;
        let distance: u64 = 5;
        let answer = sc
            .location_to_changeset_id(&ctx, Location::new(known_cs, distance))
            .await?;
        let expected_cs =
            resolve_cs_id(&ctx, &blobrepo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
        assert_eq!(answer, expected_cs);
    }

    Ok(())
}

#[fbinit::test]
async fn test_two_repos(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let blobrepo1 = Linear::getrepo_with_id(fb, RepositoryId::new(1)).await;
    let known_cs1 =
        resolve_cs_id(&ctx, &blobrepo1, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    seed(&ctx, &blobrepo1, &conns, known_cs1).await?;
    let sc1 = load_owned(&ctx, &blobrepo1, &conns).await?;

    let blobrepo2 = MergeEven::getrepo_with_id(fb, RepositoryId::new(2)).await;
    let known_cs2 =
        resolve_cs_id(&ctx, &blobrepo2, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
    seed(&ctx, &blobrepo2, &conns, known_cs2).await?;
    let sc2 = load_owned(&ctx, &blobrepo2, &conns).await?;

    let distance: u64 = 4;
    let answer = sc1
        .location_to_changeset_id(&ctx, Location::new(known_cs1, distance))
        .await?;
    let expected_cs_id =
        resolve_cs_id(&ctx, &blobrepo1, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
    assert_eq!(answer, expected_cs_id);

    let distance: u64 = 2;
    let answer = sc2
        .location_to_changeset_id(&ctx, Location::new(known_cs2, distance))
        .await?;
    let expected_cs_id =
        resolve_cs_id(&ctx, &blobrepo2, "d7542c9db7f4c77dab4b315edd328edf1514952f").await?;
    assert_eq!(answer, expected_cs_id);

    Ok(())
}

#[fbinit::test]
async fn test_on_demand_update_commit_location_to_changeset_ids(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let inner = Linear::get_inner_repo(fb).await;
    let blobrepo = inner.as_blob_repo();

    // commit modified10 (11)
    let cs11 = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    // commit 10
    let cs10 = resolve_cs_id(&ctx, &blobrepo, "a5ffa77602a066db7d5cfb9fb5823a0895717c5a").await?;
    // commit 6
    let cs6 = resolve_cs_id(&ctx, &blobrepo, "eed3a8c0ec67b6a6fe2eb3543334df3f0b4f202b").await?;

    // commit 5
    let cs5 = resolve_cs_id(&ctx, &blobrepo, "cb15ca4a43a59acff5388cea9648c162afde8372").await?;

    let sc = inner.segmented_changelog();
    let answer = try_join_all(vec![
        sc.location_to_changeset_id(&ctx, Location::new(cs10, 5)),
        sc.location_to_changeset_id(&ctx, Location::new(cs6, 1)),
        sc.location_to_changeset_id(&ctx, Location::new(cs11, 6)),
    ])
    .await?;
    assert_eq!(answer, vec![cs5, cs5, cs5]);

    // Recreate the test repo to reset the segmented changelog
    let inner = Linear::get_inner_repo(fb).await;
    let sc = inner.segmented_changelog();
    let answer = try_join_all(vec![
        sc.changeset_id_to_location(&ctx, vec![cs10], cs5),
        sc.changeset_id_to_location(&ctx, vec![cs6], cs5),
        sc.changeset_id_to_location(&ctx, vec![cs11], cs5),
        sc.changeset_id_to_location(
            &ctx,
            vec![cs11],
            mononoke_types_mocks::changesetid::ONES_CSID,
        ),
    ])
    .await?;
    assert_eq!(
        answer,
        vec![
            Some(Location::new(cs10, 5)),
            Some(Location::new(cs6, 1)),
            Some(Location::new(cs11, 6)),
            None,
        ]
    );

    Ok(())
}

#[fbinit::test]
async fn test_incremental_update_with_desync_iddag(fb: FacebookInit) -> Result<()> {
    // In this test we first build an demand update segmented changelog with an empty idmap and
    // then we reuse the idmap with another on demand update segmented changelog that starts off
    // with an empty iddag.
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    let idmap: Arc<dyn IdMap> = Arc::new(SqlIdMap::new(
        conns.0.clone(),
        Arc::new(NoReplicaLagMonitor()),
        blobrepo.get_repoid(),
        IdMapVersion(0),
    ));
    let new_sc = || {
        OnDemandUpdateSegmentedChangelog::new(
            ctx.clone(),
            blobrepo.get_repoid(),
            InProcessIdDag::new_in_process(),
            Arc::clone(&idmap),
            blobrepo.get_changeset_fetcher(),
            Arc::clone(blobrepo.bookmarks()) as Arc<dyn Bookmarks>,
            vec![Some(BOOKMARK_NAME.clone()).into()],
            None,
        )
    };

    let master_cs =
        resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;

    let initial = new_sc()?;

    let cs7 = resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
    let distance: u64 = 4;
    let answer = initial
        .location_to_changeset_id(&ctx, Location::new(master_cs, distance))
        .await?;
    assert_eq!(answer, cs7);

    let second = new_sc()?;

    let cs3 = resolve_cs_id(&ctx, &blobrepo, "607314ef579bd2407752361ba1b0c1729d08b281").await?;
    let answer = second
        .location_to_changeset_id(&ctx, Location::new(cs7, distance))
        .await?;
    assert_eq!(answer, cs3);

    let answer = second
        .location_to_changeset_id(&ctx, Location::new(master_cs, distance))
        .await?;
    assert_eq!(answer, cs7);

    Ok(())
}

#[fbinit::test]
async fn test_clone_data(fb: FacebookInit) -> Result<()> {
    // In this test we first build a dag from scratch and then we reuse the idmap in an ondemand
    // segmented changelog that starts off with an empty iddag.
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let head = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    seed(&ctx, &blobrepo, &conns, head).await?;

    let sc = load_owned(&ctx, &blobrepo, &conns).await?;

    let clone_data = sc.clone_data(&ctx).await?;

    let mut new_iddag = InProcessIdDag::new_in_process();
    new_iddag.build_segments_from_prepared_flat_segments(&clone_data.0.flat_segments)?;
    let new_owned = OwnedSegmentedChangelog::new(new_iddag, sc.idmap.clone());

    let distance: u64 = 4;
    let answer = new_owned
        .location_to_changeset_id(&ctx, Location::new(head, distance))
        .await?;
    let expected_cs =
        resolve_cs_id(&ctx, &blobrepo, "0ed509bf086fadcb8a8a5384dc3b550729b0fc17").await?;
    assert_eq!(answer, expected_cs);

    Ok(())
}

#[fbinit::test]
async fn test_caching(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let cs_to_dag_handler = CachelibHandler::create_mock();
    let dag_to_cs_handler = CachelibHandler::create_mock();
    let memcache_handler = MemcacheHandler::create_mock();
    let cache_handlers = CacheHandlers::new(
        cs_to_dag_handler.clone(),
        dag_to_cs_handler.clone(),
        memcache_handler.clone(),
    );

    // It's easier to reason about cache hits and sets when the dag is already built
    let head = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    seed(&ctx, &blobrepo, &conns, head).await?;

    let iddag = load_iddag(&ctx, &blobrepo, &conns).await?;
    let sc_version = load_sc_version(&ctx, blobrepo.get_repoid(), &conns).await?;
    let idmap = IdMapFactory::new(
        conns.0.clone(),
        Arc::new(NoReplicaLagMonitor()),
        blobrepo.get_repoid(),
    )
    .with_cache_handlers(cache_handlers)
    .for_server(&ctx, sc_version.idmap_version, &iddag)?;
    let sc = OwnedSegmentedChangelog::new(iddag, idmap);

    let distance: u64 = 1;
    let _ = sc
        .location_to_changeset_id(&ctx, Location::new(head, distance))
        .await?;

    let cs_to_dag_stats = || {
        cs_to_dag_handler
            .mock_store()
            .expect("mock handler has mock store")
            .stats()
    };
    let dag_to_cs_stats = || {
        dag_to_cs_handler
            .mock_store()
            .expect("mock handler has mock store")
            .stats()
    };

    assert_eq!(cs_to_dag_stats().gets, 1);
    assert_eq!(cs_to_dag_stats().misses, 1);
    assert_eq!(cs_to_dag_stats().sets, 1);

    assert_eq!(dag_to_cs_stats().gets, 1);
    assert_eq!(dag_to_cs_stats().misses, 1);
    assert_eq!(dag_to_cs_stats().sets, 1);

    let _ = sc
        .location_to_changeset_id(&ctx, Location::new(head, distance))
        .await?;

    assert_eq!(cs_to_dag_stats().gets, 2);
    assert_eq!(cs_to_dag_stats().hits, 1);

    assert_eq!(dag_to_cs_stats().gets, 2);
    assert_eq!(dag_to_cs_stats().hits, 1);

    Ok(())
}

#[fbinit::test]
async fn test_periodic_update(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let bookmark_name = BookmarkName::new("periodic_update")?;

    let start_hg_id = "607314ef579bd2407752361ba1b0c1729d08b281";
    let start_cs = resolve_cs_id(&ctx, &blobrepo, start_hg_id).await?;
    set_bookmark(fb, &blobrepo, start_hg_id, bookmark_name.clone()).await;

    tokio::time::pause(); // TODO: pause only works with the `current_thread` Runtime.

    let on_demand = OnDemandUpdateSegmentedChangelog::new(
        ctx.clone(),
        blobrepo.get_repoid(),
        InProcessIdDag::new_in_process(),
        Arc::new(ConcurrentMemIdMap::new()),
        blobrepo.get_changeset_fetcher(),
        Arc::clone(blobrepo.bookmarks()) as Arc<dyn Bookmarks>,
        vec![Some(bookmark_name.clone()).into()],
        None,
    )?;
    let sc =
        Arc::new(on_demand).with_periodic_update_to_master_bookmark(&ctx, Duration::from_secs(5));

    tokio::time::advance(Duration::from_secs(10)).await;
    // wait time is over and the first tick is ready to be scheduled
    sc.wait_for_update().await;

    // We assume that clone_data will not update the graph in any form.
    assert_eq!(sc.head(&ctx).await?, start_cs);

    let new_hg_id = "79a13814c5ce7330173ec04d279bf95ab3f652fb";
    let new_cs = resolve_cs_id(&ctx, &blobrepo, new_hg_id).await?;
    set_bookmark(fb, &blobrepo, new_hg_id, bookmark_name.clone()).await;

    tokio::time::advance(Duration::from_secs(5)).await;
    // second tick is ready to be scheduled
    sc.wait_for_update().await;
    assert_eq!(sc.head(&ctx).await?, new_cs);

    Ok(())
}

#[fbinit::test]
async fn test_seeder_tailer_and_load(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let start_hg_id = "607314ef579bd2407752361ba1b0c1729d08b281"; // commit 4
    let start_cs_id = resolve_cs_id(&ctx, &blobrepo, start_hg_id).await?;

    seed(&ctx, &blobrepo, &conns, start_cs_id).await?;

    let sc = load_owned(&ctx, &blobrepo, &conns).await?;
    assert_eq!(sc.head(&ctx).await?, start_cs_id);

    let tailer = new_tailer_for_tailing(&blobrepo, &conns).await?;
    let _ = tailer.once(&ctx, false).await?;
    let sc = load_owned(&ctx, &blobrepo, &conns).await?;
    let master = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    assert_eq!(sc.head(&ctx).await?, master);

    Ok(())
}

#[fbinit::test]
async fn test_periodic_reload(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Arc::new(Linear::getrepo(fb).await);
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let start_hg_id = "607314ef579bd2407752361ba1b0c1729d08b281"; // commit 4
    let start_cs_id = resolve_cs_id(&ctx, &blobrepo, start_hg_id).await?;

    seed(&ctx, &blobrepo, &conns, start_cs_id).await?;

    tokio::time::pause();
    let manager = get_manager(&blobrepo, &conns, vec![], SegmentedChangelogType::Owned).await?;
    let sc = PeriodicReloadSegmentedChangelog::start_from_manager(
        &ctx,
        Duration::from_secs(5),
        manager,
        blobrepo.name().to_string(),
    )
    .await?;

    assert_eq!(sc.head(&ctx).await?, start_cs_id);

    // Try waiting for segmented changelog update without tailer running.  This
    // should fail as we don't update the SC unless there's an update.
    assert!(
        tokio::time::timeout(Duration::from_secs(15), sc.wait_for_update())
            .await
            .is_err()
    );

    // Start the tailer.
    let tailer = new_tailer_for_tailing(&blobrepo, &conns).await?;
    let _ = tailer.once(&ctx, false).await?;
    // There was an update - wait_for_update should return.
    tokio::time::timeout(Duration::from_secs(15), sc.wait_for_update()).await?;
    let master = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;
    assert_eq!(sc.head(&ctx).await?, master);

    // No updates should happen afterwards because there are no new commits.
    assert!(
        tokio::time::timeout(Duration::from_secs(45), sc.wait_for_update())
            .await
            .is_err()
    );

    // Force the reload should trigger even if there are not updates at all.
    let tunables_override = Arc::new(tunables::MononokeTunables::default());
    tunables_override.update_by_repo_ints(&hashmap! {
        blobrepo.name().clone() => hashmap! {
            "segmented_changelog_force_reload".to_string() => 2,
        },
    });
    tunables_override.update_ints(&hashmap! {
        "segmented_changelog_force_reload_jitter_secs".to_string() => 5,
    });
    override_tunables(Some(tunables_override.clone()));
    tokio::time::timeout(Duration::from_secs(45), sc.wait_for_update()).await?;
    override_tunables(None);
    Ok(())
}

#[fbinit::test]
async fn test_manager_check_if_indexed(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Arc::new(Linear::getrepo(fb).await);
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;

    let start_cs_id =
        resolve_cs_id(&ctx, &blobrepo, "607314ef579bd2407752361ba1b0c1729d08b281").await?;
    let master = resolve_cs_id(&ctx, &blobrepo, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await?;

    seed(&ctx, &blobrepo, &conns, start_cs_id).await?;

    tokio::time::pause();
    let manager = get_manager(&blobrepo, &conns, vec![], SegmentedChangelogType::Owned).await?;

    // Changeset is not part of segmented changelog before tailer run.
    let version = manager.latest_version(&ctx).await?;
    assert!(
        !manager
            .check_if_changeset_indexed(&ctx, &version, master)
            .await?
    );

    let tailer = new_tailer_for_tailing(&blobrepo, &conns).await?;
    let _ = tailer.once(&ctx, false).await?;
    // Changeset is part of segmented changelog after tailer run.
    let version = manager.latest_version(&ctx).await?;
    assert!(
        manager
            .check_if_changeset_indexed(&ctx, &version, master)
            .await?
    );
    Ok(())
}

#[fbinit::test]
async fn test_mismatched_heads(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let inner = BranchEven::get_inner_repo(fb).await;
    let blobrepo = inner.as_blob_repo();

    let dag = inner.segmented_changelog();
    let h1 = resolve_cs_id(&ctx, &blobrepo, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await?;
    let h1_parent =
        resolve_cs_id(&ctx, &blobrepo, "b65231269f651cfe784fd1d97ef02a049a37b8a0").await?;

    assert_eq!(
        dag.changeset_id_to_location(&ctx, vec![h1], h1_parent)
            .await?,
        Some(Location::new(h1, 1))
    );

    // should fail with the small limit for traversing from client heads
    let h2 = resolve_cs_id(&ctx, &blobrepo, "16839021e338500b3cf7c9b871c8a07351697d68").await?;

    let tunables = tunables::MononokeTunables::default();
    tunables.update_ints(&hashmap! {
        "segmented_changelog_client_max_commits_to_traverse".to_string() => 2,
    });
    let f = dag.changeset_id_to_location(&ctx, vec![h1, h2], h1_parent);

    let err = with_tunables_async(tunables, f.boxed())
        .await
        .err()
        .unwrap();
    assert!(err.is::<crate::MismatchedHeadsError>());

    // should succeed as the client head not far from the commits in SC IdMap
    let h2 = resolve_cs_id(&ctx, &blobrepo, "16839021e338500b3cf7c9b871c8a07351697d68").await?;

    let tunables = tunables::MononokeTunables::default();
    tunables.update_ints(&hashmap! {
        "segmented_changelog_client_max_commits_to_traverse".to_string() => 100,
    });
    let f = dag.changeset_id_to_location(&ctx, vec![h1, h2], h1_parent);

    assert_eq!(
        with_tunables_async(tunables, f.boxed()).await?,
        Some(Location::new(h1, 1))
    );

    Ok(())
}
#[fbinit::test]
async fn test_prefetched_seeding(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    let repo_id = blobrepo.get_repoid();

    let first_cs_entry =
        fetch_cs_entry(&ctx, &blobrepo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await?;
    let second_cs_entry =
        fetch_cs_entry(&ctx, &blobrepo, "3e0e761030db6e479a7fb58b12881883f9f8c63f").await?;
    let last_cs =
        resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;

    seed_with_prefetched(
        &ctx,
        &blobrepo,
        &conns,
        vec![last_cs],
        Some(vec![first_cs_entry, second_cs_entry]),
    )
    .await?;

    // Check that new entries were added to the idmap
    let idmap = load_idmap(&ctx, repo_id, &conns).await?;
    assert!(idmap.find_dag_id(&ctx, last_cs).await?.is_some());

    Ok(())
}

#[fbinit::test]
async fn test_seeding_with_included_bonsais(fb: FacebookInit) -> Result<()> {
    let ctx = CoreContext::test_mock(fb);
    let blobrepo = Linear::getrepo(fb).await;
    let conns = SegmentedChangelogSqlConnections::with_sqlite_in_memory()?;
    let repo_id = blobrepo.get_repoid();

    let first_cs_id =
        resolve_cs_id(&ctx, &blobrepo, "2d7d4ba9ce0a6ffd222de7785b249ead9c51c536").await?;
    let last_cs =
        resolve_cs_id(&ctx, &blobrepo, "d0a361e9022d226ae52f689667bd7d212a19cfe0").await?;

    let cs_id_to_include = CreateCommitContext::new(&ctx, &blobrepo, vec![first_cs_id])
        .add_file("somefile", "file")
        .commit()
        .await?;

    seed_with_prefetched(
        &ctx,
        &blobrepo,
        &conns,
        vec![last_cs, cs_id_to_include],
        None,
    )
    .await?;

    // Check that cs_id_to_include was added to idmap
    let idmap = load_idmap(&ctx, repo_id, &conns).await?;
    let dag_id = idmap.find_dag_id(&ctx, cs_id_to_include).await?.unwrap();
    let iddag = load_iddag(&ctx, &blobrepo, &conns).await?;
    assert!(iddag.find_flat_segment_including_id(dag_id)?.is_some());

    Ok(())
}
