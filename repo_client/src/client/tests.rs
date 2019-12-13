/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::*;
use blobstore::Loadable;
use fbinit::FacebookInit;
use fixtures::many_files_dirs;
use futures_preview::compat::Future01CompatExt;
use maplit::hashset;

use mononoke_types_mocks::changesetid::ONES_CSID;
use std::collections::HashSet;
use tokio_preview as tokio;

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
    let json_config = String::from(
        r#"
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
}"#,
    );

    let push_action = PostResolveAction::Push(PostResolvePush {
        changegroup_id: None,
        bookmark_pushes: Vec::new(),
        maybe_raw_bundle2_id: None,
        non_fast_forward_policy: NonFastForwardPolicy::Allowed,
        uploaded_bonsais: HashSet::new(),
    });
    let infinitepush_action = PostResolveAction::InfinitePush(PostResolveInfinitePush {
        changegroup_id: None,
        bookmark_push: InfiniteBookmarkPush {
            name: BookmarkName::new("").unwrap(),
            create: true,
            force: true,
            old: None,
            new: ONES_CSID,
        },
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
        uploaded_hg_changeset_ids: HashSet::new(),
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

    let config_loader = ConfigLoader::default_content(json_config);
    for action in [&push_action, &pushrebase_action, &bookmark_only_action].iter() {
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-4), Some(&config_loader), action).unwrap(),
            false,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-3), Some(&config_loader), action).unwrap(),
            true,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-2), Some(&config_loader), action).unwrap(),
            true,
        );
        assert_eq!(
            maybe_pushredirect_action(RepositoryId::new(-1), Some(&config_loader), action).unwrap(),
            false,
        );
    }
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-4),
            Some(&config_loader),
            &infinitepush_action
        )
        .unwrap(),
        false,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-3),
            Some(&config_loader),
            &infinitepush_action
        )
        .unwrap(),
        true,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-2),
            Some(&config_loader),
            &infinitepush_action
        )
        .unwrap(),
        false,
    );
    assert_eq!(
        maybe_pushredirect_action(
            RepositoryId::new(-1),
            Some(&config_loader),
            &infinitepush_action
        )
        .unwrap(),
        true,
    );
}

#[fbinit::test]
async fn get_changed_manifests_stream_test(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb);

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
async fn get_changed_manifests_stream_test_depth(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb);

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
async fn get_changed_manifests_stream_test_base_path(fb: FacebookInit) -> Result<(), Error> {
    let ctx = CoreContext::test_mock(fb);
    let repo = many_files_dirs::getrepo(fb);

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
        repo.get_manifest_by_nodeid(ctx.clone(), *hash)
            .compat()
            .await?;
    }
    Ok(fetched_mfs)
}
