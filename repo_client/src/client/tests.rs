/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License found in the LICENSE file in the root
 * directory of this source tree.
 */

use super::*;
use maplit::hashset;
use mononoke_types_mocks::changesetid::ONES_CSID;

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
    use bundle2_resolver::*;
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
        uploaded_hg_bonsai_map: HashMap::new(),
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
        uploaded_hg_bonsai_map: HashMap::new(),
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
        maybe_raw_bundle2_id: None,
        maybe_pushvars: None,
        commonheads: CommonHeads { heads: Vec::new() },
        uploaded_hg_bonsai_map: HashMap::new(),
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
