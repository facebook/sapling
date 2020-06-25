/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use super::*;
use blobstore::Loadable;
use cached_config::ConfigHandle;
use fbinit::FacebookInit;
use fixtures::many_files_dirs;
use futures::compat::Future01CompatExt;
use hooks::HookManager;
use hooks_content_stores::InMemoryFileContentFetcher;
use manifest::{Entry, ManifestOps};
use maplit::hashset;
use mercurial_types::HgFileNodeId;
use metaconfig_types::{HookManagerParams, InfinitepushParams, LfsParams, PushrebaseParams};
use mononoke_repo::MononokeRepo;
use mutable_counters::SqlMutableCounters;
use repo_read_write_status::RepoReadWriteFetcher;
use scuba_ext::ScubaSampleBuilder;
use skiplist::SkiplistIndex;
use sql_construct::SqlConstruct;
use tests_utils::CreateCommitContext;

use mononoke_types_mocks::changesetid::ONES_CSID;
use std::collections::HashSet;

#[test]
fn test_parsing_caps_simple() {
    assert_eq!(
        parse_utf8_getbundle_caps(b"cap"),
        Some((String::from("cap"), HashMap::new())),
    );

    let caps = b"bundle2=HG20";

    assert_eq!(
        parse_utf8_getbundle_caps(caps),
        Some((
            String::from("bundle2"),
            hashmap! { "HG20".to_string() => hashset!{} }
        )),
    );

    let caps = b"bundle2=HG20%0Ab2x%253Ainfinitepush%0Ab2x%253Ainfinitepushscratchbookmarks\
        %0Ab2x%253Arebase%0Abookmarks%0Achangegroup%3D01%2C02%0Adigests%3Dmd5%2Csha1%2Csha512%0A\
        error%3Dabort%2Cunsupportedcontent%2Cpushraced%2Cpushkey%0Ahgtagsfnodes%0Alistkeys%0A\
        pushkey%0Aremote-changegroup%3Dhttp%2Chttps%0Aremotefilelog%3DTrue%0Atreemanifest%3DTrue%0Atreeonly%3DTrue";

    assert_eq!(
        parse_utf8_getbundle_caps(caps),
        Some((
            String::from("bundle2"),
            hashmap! {
                "HG20".to_string() => hashset!{},
                "b2x:rebase".to_string() => hashset!{},
                "digests".to_string() => hashset!{"md5".to_string(), "sha512".to_string(), "sha1".to_string()},
                "listkeys".to_string() => hashset!{},
                "remotefilelog".to_string() => hashset!{"True".to_string()},
                "hgtagsfnodes".to_string() => hashset!{},
                "bookmarks".to_string() => hashset!{},
                "b2x:infinitepushscratchbookmarks".to_string() => hashset!{},
                "treeonly".to_string() => hashset!{"True".to_string()},
                "pushkey".to_string() => hashset!{},
                "error".to_string() => hashset!{
                    "pushraced".to_string(),
                    "pushkey".to_string(),
                    "unsupportedcontent".to_string(),
                    "abort".to_string(),
                },
                "b2x:infinitepush".to_string() => hashset!{},
                "changegroup".to_string() => hashset!{"01".to_string(), "02".to_string()},
                "remote-changegroup".to_string() => hashset!{"http".to_string(), "https".to_string()},
                "treemanifest".to_string() => hashset!{"True".to_string()},
            }
        )),
    );
}

#[test]
fn test_pushredirect_config() {
    use unbundle::*;
    // This ends up being exhaustive
    let json_config = r#"
{
  "per_repo": {
    "-4": {
        "draft_push": false,
        "public_push": false
    },
    "-3": {
        "draft_push": true,
        "public_push": true
    },
    "-2": {
        "draft_push": false,
        "public_push": true
    },
    "-1": {
        "draft_push": true,
        "public_push": false
    }
  }
}"#;

    let push_action = PostResolveAction::Push(PostResolvePush {
        changegroup_id: None,
        bookmark_pushes: Vec::new(),
        maybe_raw_bundle2_id: None,
        non_fast_forward_policy: NonFastForwardPolicy::Allowed,
        uploaded_bonsais: HashSet::new(),
    });
    let infinitepush_action = PostResolveAction::InfinitePush(PostResolveInfinitePush {
        changegroup_id: None,
        maybe_bookmark_push: Some(InfiniteBookmarkPush {
            name: BookmarkName::new("").unwrap(),
            create: true,
            force: true,
            old: None,
            new: ONES_CSID,
        }),
        maybe_raw_bundle2_id: None,
        uploaded_bonsais: HashSet::new(),
    });
    let pushrebase_action = PostResolveAction::PushRebase(PostResolvePushRebase {
        any_merges: true,
        bookmark_push_part_id: None,
        bookmark_spec: PushrebaseBookmarkSpec::ForcePushrebase(PlainBookmarkPush {
            part_id: 0,
            name: BookmarkName::new("").unwrap(),
            old: None,
            new: None,
        }),
        maybe_hg_replay_data: None,
        maybe_pushvars: None,
        commonheads: CommonHeads { heads: Vec::new() },
        uploaded_bonsais: HashSet::new(),
    });
    let bookmark_only_action =
        PostResolveAction::BookmarkOnlyPushRebase(PostResolveBookmarkOnlyPushRebase {
            bookmark_push: PlainBookmarkPush {
                part_id: 0,
                name: BookmarkName::new("").unwrap(),
                old: None,
                new: None,
            },
            maybe_raw_bundle2_id: None,
            non_fast_forward_policy: NonFastForwardPolicy::Allowed,
        });

    let config_handler = ConfigHandle::from_json(&json_config).unwrap();
    for action in [&push_action, &pushrebase_action, &bookmark_only_action].iter() {
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-4), Some(&config_handler), action)
                .unwrap(),
            false,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-3), Some(&config_handler), action)
                .unwrap(),
            true,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-2), Some(&config_handler), action)
                .unwrap(),
            true,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-1), Some(&config_handler), action)
                .unwrap(),
            false,
        );
    }
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-4),
            Some(&config_handler),
            &infinitepush_action
        )
        .unwrap(),
        false,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-3),
            Some(&config_handler),
            &infinitepush_action
        )
        .unwrap(),
        true,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-2),
            Some(&config_handler),
            &infinitepush_action
        )
        .unwrap(),
        false,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-1),
            Some(&config_handler),
            &infinitepush_action
        )
        .unwrap(),
        true,
    );
}

#[fbinit::test]
fn get_changed_manifests_stream_test(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio_compat::runtime::Runtime::new()?;
    runtime.block_on_std(get_changed_manifests_stream_test_impl(fb))
}

async fn get_changed_manifests_stream_test_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb).await;

    // Commit that has only dir2 directory
    let root_mf_id = HgChangesetId::from_str("051946ed218061e925fb120dac02634f9ad40ae2")?
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?
        .manifestid();

    let fetched_mfs = fetch_mfs(
        ctx.clone(),
        &repo,
        root_mf_id,
        HgManifestId::new(NULL_HASH),
        None,
        65536,
    )
    .await?;

    let mut res = fetched_mfs
        .into_iter()
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    res.sort();
    let mut expected = vec![None, Some(MPath::new("dir2")?)];
    expected.sort();
    assert_eq!(res, expected);

    // Now commit that added a few files and directories

    let root_mf_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4")?
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?
        .manifestid();

    let base_root_mf_id = HgChangesetId::from_str("2f866e7e549760934e31bf0420a873f65100ad63")?
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?
        .manifestid();

    let fetched_mfs =
        fetch_mfs(ctx.clone(), &repo, root_mf_id, base_root_mf_id, None, 65536).await?;

    let mut res = fetched_mfs
        .into_iter()
        .map(|(_, path)| path)
        .collect::<Vec<_>>();
    res.sort();
    let mut expected = vec![
        None,
        Some(MPath::new("dir1")?),
        Some(MPath::new("dir1/subdir1")?),
        Some(MPath::new("dir1/subdir1/subsubdir1")?),
        Some(MPath::new("dir1/subdir1/subsubdir2")?),
    ];
    expected.sort();
    assert_eq!(res, expected);

    Ok(())
}

#[fbinit::test]
fn get_changed_manifests_stream_test_depth(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio_compat::runtime::Runtime::new()?;
    runtime.block_on_std(get_changed_manifests_stream_test_depth_impl(fb))
}

async fn get_changed_manifests_stream_test_depth_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb).await;

    let root_mf_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4")?
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?
        .manifestid();

    let base_mf_id = HgManifestId::new(NULL_HASH);
    let fetched_mfs = fetch_mfs(ctx.clone(), &repo, root_mf_id, base_mf_id, None, 65536).await?;

    let paths = fetched_mfs
        .into_iter()
        .map(|(_, path)| path)
        .collect::<Vec<_>>();

    let max_depth = paths
        .iter()
        .map(|path| match path {
            Some(path) => path.num_components(),
            None => 0,
        })
        .max()
        .unwrap();

    for depth in 0..max_depth + 1 {
        println!("depth: {}", depth);
        let fetched_mfs =
            fetch_mfs(ctx.clone(), &repo, root_mf_id, base_mf_id, None, depth).await?;
        let mut actual = fetched_mfs
            .into_iter()
            .map(|(_, path)| path)
            .collect::<Vec<_>>();
        actual.sort();
        let iter = paths.clone().into_iter();
        // We have a weird hard-coded behaviour for depth=1 that we are preserving for now
        let mut expected: Vec<_> = if depth == 1 {
            let expected: Vec<_> = iter.filter(|path| path.is_none()).collect();
            assert_eq!(expected.len(), 1);
            expected
        } else {
            iter.filter(|path| match path {
                Some(path) => path.num_components() <= depth,
                None => true,
            })
            .collect()
        };
        expected.sort();
        assert_eq!(actual, expected);
    }

    Ok(())
}

#[fbinit::test]
fn get_changed_manifests_stream_test_base_path(fb: FacebookInit) -> Result<(), Error> {
    let mut runtime = tokio_compat::runtime::Runtime::new()?;
    runtime.block_on_std(get_changed_manifests_stream_test_base_path_impl(fb))
}

async fn get_changed_manifests_stream_test_base_path_impl(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb).await;

    let root_mf_id = HgChangesetId::from_str("d261bc7900818dea7c86935b3fb17a33b2e3a6b4")?
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?
        .manifestid();

    let base_mf_id = HgManifestId::new(NULL_HASH);
    let fetched_mfs = fetch_mfs(ctx.clone(), &repo, root_mf_id, base_mf_id, None, 65536).await?;

    for (hash, path) in &fetched_mfs {
        println!("base path: {:?}", path);
        let mut actual =
            fetch_mfs(ctx.clone(), &repo, *hash, base_mf_id, path.clone(), 65536).await?;
        actual.sort();

        let mut expected: Vec<_> = fetched_mfs
            .clone()
            .into_iter()
            .filter(|(_, curpath)| match &path {
                Some(path) => {
                    let elems = MPath::iter_opt(curpath.as_ref());
                    path.is_prefix_of(elems)
                }
                None => true,
            })
            .collect();
        expected.sort();
        assert_eq!(actual, expected);
    }

    Ok(())
}

#[fbinit::compat_test]
async fn test_lfs_rollout(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = blobrepo_factory::new_memblob_empty(None)?;
    let commit = CreateCommitContext::new_root(&ctx, &repo)
        .add_file("largefile", "11111_11111")
        .commit()
        .await?;

    let hg_cs_id = repo
        .get_hg_from_bonsai_changeset(ctx.clone(), commit)
        .compat()
        .await?;

    let hg_cs = hg_cs_id
        .load(ctx.clone(), &repo.get_blobstore())
        .compat()
        .await?;

    let path = MPath::new("largefile")?;
    let maybe_entry = hg_cs
        .manifestid()
        .find_entry(ctx.clone(), repo.get_blobstore(), Some(path.clone()))
        .compat()
        .await?
        .unwrap();

    let filenode_id = match maybe_entry {
        Entry::Leaf((_, filenode_id)) => filenode_id,
        Entry::Tree(_) => {
            panic!("should be a leaf");
        }
    };
    assert_eq!(
        run_and_check_if_lfs(&ctx, &repo, &path, &filenode_id, LfsParams::default()).await?,
        false
    );

    // Rollout percentage is 100 and threshold is set - enable lfs
    let lfs_params = LfsParams {
        threshold: Some(5),
        rollout_percentage: 100,
        ..Default::default()
    };
    assert_eq!(
        run_and_check_if_lfs(&ctx, &repo, &path, &filenode_id, lfs_params).await?,
        true
    );

    // Rollout percentage is 0 - no lfs is enabled
    let lfs_params = LfsParams {
        threshold: Some(5),
        rollout_percentage: 0,
        ..Default::default()
    };
    assert_eq!(
        run_and_check_if_lfs(&ctx, &repo, &path, &filenode_id, lfs_params).await?,
        false
    );

    // Rollout percentage is 100, but threshold is too high
    let lfs_params = LfsParams {
        threshold: Some(500),
        rollout_percentage: 100,
        ..Default::default()
    };
    assert_eq!(
        run_and_check_if_lfs(&ctx, &repo, &path, &filenode_id, lfs_params).await?,
        false
    );
    Ok(())
}

async fn run_and_check_if_lfs(
    ctx: &CoreContext,
    repo: &BlobRepo,
    path: &MPath,
    filenode_id: &HgFileNodeId,
    lfs_params: LfsParams,
) -> Result<bool, Error> {
    let pushrebase_params = PushrebaseParams::default();

    let mononoke_repo = MononokeRepo::new(
        ctx.fb,
        ctx.logger().clone(),
        repo.clone(),
        &pushrebase_params,
        vec![],
        Arc::new(HookManager::new(
            ctx.fb,
            Box::new(InMemoryFileContentFetcher::new()),
            HookManagerParams {
                disable_acl_checker: true,
            },
            ScubaSampleBuilder::with_discard(),
        )),
        None,
        lfs_params,
        RepoReadWriteFetcher::new(
            None,
            RepoReadOnly::ReadOnly("".to_string()),
            "repo".to_string(),
        ),
        InfinitepushParams::default(),
        0,
        Arc::new(SkiplistIndex::new()),
        Arc::new(SqlMutableCounters::with_sqlite_in_memory()?),
    )
    .await?;

    let logging = LoggingContainer::new(ctx.logger().clone(), ScubaSampleBuilder::with_discard());

    let noop_wireproto =
        WireprotoLogging::new(ctx.fb, mononoke_repo.reponame().clone(), None, None, None)?;

    let repo_client = RepoClient::new(
        mononoke_repo,
        ctx.session().clone(),
        logging,
        100,   // hash validation percentage
        false, // Don't preserve raw bundle 2 (we don't push)
        false, // Don't allow pushes (we don't push)
        true,  // Support bundle2_listkeys
        Arc::new(noop_wireproto),
        None, // No PushRedirectorArgs
        None, // Don't listen to LiveCommitSyncConfig
    );

    let bytes = repo_client
        .getpackv2(stream::iter_ok(vec![(path.clone(), vec![*filenode_id])]).boxify())
        .concat2()
        .compat()
        .await?;

    let lfs_url: &[u8] = b"version https://git-lfs.github.com/spec/v1";

    let found = bytes.windows(lfs_url.len()).any(|w| w == lfs_url);

    Ok(found)
}

async fn fetch_mfs(
    ctx: CoreContext,
    repo: &BlobRepo,
    root_mf_id: HgManifestId,
    base_root_mf_id: HgManifestId,
    base_path: Option<MPath>,
    depth: usize,
) -> Result<Vec<(HgManifestId, Option<MPath>)>, Error> {
    let fetched_mfs = get_changed_manifests_stream(
        ctx.clone(),
        &repo,
        root_mf_id,
        base_root_mf_id,
        base_path,
        depth,
    )
    .collect()
    .compat()
    .await?;

    // Make sure that Manifest ids are present in the repo
    for (hash, _) in &fetched_mfs {
        hash.load(ctx.clone(), repo.blobstore()).compat().await?;
    }
    Ok(fetched_mfs)
}
