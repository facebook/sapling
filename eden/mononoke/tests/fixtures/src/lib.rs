/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::str::FromStr;

use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bookmarks::BookmarkKey;
use bookmarks::BookmarkUpdateReason;
use bytes::Bytes;
use context::CoreContext;
use fbinit::FacebookInit;
use filestore::StoreRequest;
use futures::stream;
use mercurial_derivation::DeriveHgChangeset;
use mercurial_types::HgChangesetId;
use mercurial_types::NonRootMPath;
use mononoke_types::BonsaiChangeset;
use mononoke_types::BonsaiChangesetMut;
use mononoke_types::ChangesetId;
use mononoke_types::DateTime;
use mononoke_types::FileChange;
use mononoke_types::FileType;
use mononoke_types::GitLfs;
use mononoke_types::RepositoryId;
use sorted_vector_map::SortedVectorMap;
use test_repo_factory::TestRepoFactory;
use test_repo_factory::TestRepoFactoryBuilder;
use tests_utils::Repo;
use tests_utils::drawdag::extend_from_dag_with_actions;

pub async fn store_files(
    ctx: &CoreContext,
    files: BTreeMap<&str, Option<&str>>,
    repo: &impl Repo,
) -> SortedVectorMap<NonRootMPath, FileChange> {
    let mut res = BTreeMap::new();

    for (path, content) in files {
        let path = NonRootMPath::new(path).unwrap();
        match content {
            Some(content) => {
                let content = Bytes::copy_from_slice(content.as_bytes());
                let size = content.len() as u64;
                let metadata = filestore::store(
                    repo.repo_blobstore(),
                    *repo.filestore_config(),
                    ctx,
                    &StoreRequest::new(size),
                    stream::once(async { Ok(content) }),
                )
                .await
                .unwrap();
                let file_change = FileChange::tracked(
                    metadata.content_id,
                    FileType::Regular,
                    size,
                    None,
                    GitLfs::FullContent,
                );
                res.insert(path, file_change);
            }
            None => {
                res.insert(path, FileChange::Deletion);
            }
        }
    }
    res.into()
}

pub async fn set_bookmark(
    fb: FacebookInit,
    repo: &impl Repo,
    hg_cs_id: &str,
    bookmark: BookmarkKey,
) {
    let ctx = CoreContext::test_mock(fb);
    let hg_cs_id = HgChangesetId::from_str(hg_cs_id).unwrap();
    let bcs_id = repo
        .bonsai_hg_mapping()
        .get_bonsai_from_hg(&ctx, hg_cs_id)
        .await
        .unwrap();
    let mut txn = repo.bookmarks().create_transaction(ctx.clone());
    txn.force_set(&bookmark, bcs_id.unwrap(), BookmarkUpdateReason::TestMove)
        .unwrap();
    txn.commit().await.unwrap();
}

#[async_trait]
pub trait TestRepoFixture {
    const REPO_NAME: &'static str;

    const DAG: &'static str = "";

    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        let ctx = CoreContext::test_mock(fb);
        let (commits, dag) = extend_from_dag_with_actions(&ctx, repo, Self::DAG).await?;
        for cs_id in commits.values() {
            repo.derive_hg_changeset(&ctx, *cs_id).await.unwrap();
        }
        Ok((commits, dag))
    }

    async fn get_repo_and_dag<
        R: Repo + for<'builder> facet::AsyncBuildable<'builder, TestRepoFactoryBuilder<'builder>>,
    >(
        fb: FacebookInit,
    ) -> (
        R,
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    ) {
        let repo: R = TestRepoFactory::new(fb)
            .unwrap()
            .with_id(RepositoryId::new(0))
            .with_name(Self::REPO_NAME.to_string())
            .build()
            .await
            .unwrap();
        let (commits, dag) = Self::init_repo(fb, &repo).await.unwrap();
        (repo, commits, dag)
    }

    async fn get_repo<
        R: Repo + for<'builder> facet::AsyncBuildable<'builder, TestRepoFactoryBuilder<'builder>>,
    >(
        fb: FacebookInit,
    ) -> R {
        Self::get_repo_with_id(fb, RepositoryId::new(0)).await
    }

    async fn get_repo_with_id<
        R: Repo + for<'builder> facet::AsyncBuildable<'builder, TestRepoFactoryBuilder<'builder>>,
    >(
        fb: FacebookInit,
        id: RepositoryId,
    ) -> R {
        let repo: R = TestRepoFactory::new(fb)
            .unwrap()
            .with_id(id)
            .with_name(Self::REPO_NAME.to_string())
            .build()
            .await
            .unwrap();
        Self::init_repo(fb, &repo).await.unwrap();
        repo
    }
}

pub struct Linear;

#[async_trait]
impl TestRepoFixture for Linear {
    const REPO_NAME: &'static str = "linear";

    const DAG: &'static str = r#"
        # default_files: false
        # bookmark: K master
        # author: * "Jeremy Fitzhardinge <jsgf@fb.com>"

        K  # message: K "modified 10"
        |  # author_date: K "2017-08-29 14:22:41-07:00"
        |  # modify: K 10 "modified10\n"
        |
        J  # message: J "added 10"
        |  # author_date: J "2017-08-29 14:22:41-07:00"
        |  # modify: J 10 "10\n"
        |  # modify: J files "1\n2\n3\n4\n5\n6\n7\n8\n9\n10\n"
        |
        I  # message: I "added 9"
        |  # author_date: I "2017-08-29 14:22:41-07:00"
        |  # modify: I 9 "9\n"
        |  # modify: I files "1\n2\n3\n4\n5\n6\n7\n8\n9\n"
        |
        H  # message: H "added 8"
        |  # author_date: H "2017-08-29 14:22:41-07:00"
        |  # modify: H 8 "8\n"
        |  # modify: H files "1\n2\n3\n4\n5\n6\n7\n8\n"
        |
        G  # message: G "added 7"
        |  # author_date: G "2017-08-29 14:22:40-07:00"
        |  # modify: G 7 "7\n"
        |  # modify: G files "1\n2\n3\n4\n5\n6\n7\n"
        |
        F  # message: F "added 6"
        |  # author_date: F "2017-08-29 14:22:40-07:00"
        |  # modify: F 6 "6\n"
        |  # modify: F files "1\n2\n3\n4\n5\n6\n"
        |
        E  # message: E "added 5"
        |  # author_date: E "2017-08-29 14:22:39-07:00"
        |  # modify: E 5 "5\n"
        |  # modify: E files "1\n2\n3\n4\n5\n"
        |
        D  # message: D "added 4"
        |  # author_date: D "2017-08-29 14:22:39-07:00"
        |  # modify: D 4 "4\n"
        |  # modify: D files "1\n2\n3\n4\n"
        |
        C  # message: C "added 3"
        |  # author_date: C "2017-08-29 14:22:38-07:00"
        |  # modify: C 3 "3\n"
        |  # modify: C files "1\n2\n3\n"
        |
        B  # message: B "added 2"
        |  # author_date: B "2017-08-29 14:22:38-07:00"
        |  # modify: B 2 "2\n"
        |  # modify: B files "1\n2\n"
        |
        A  # message: A "added 1"
           # author_date: A "2017-08-29 14:22:38-07:00"
           # modify: A 1 "1\n"
           # modify: A files "1\n"
    "#;
}

pub struct BranchEven;

#[async_trait]
impl TestRepoFixture for BranchEven {
    const REPO_NAME: &'static str = "branch_even";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: G master

        G  # message: G "Replace the base"
        |  # author_date: G "2017-09-26 07:11:02-07:00"
        |  # modify: G "base" "branch1\n"
        |
        |  F  # message: F "I think 4 is a nice number"
        |  |  # author_date: F "2017-09-26 07:10:41-07:00"
        |  |  # modify: F "branch" "4\n"
        |  |
        |  E  # message: E "Add one"
        |  |  # author_date: E "2017-09-26 05:57:08-07:00"
        |  |  # modify: E "branch" "3\n"
        |  |
        D  |  # message: D "Doubled"
        |  |  # author_date: D "2017-09-26 05:56:52-07:00"
        |  |  # modify: D "branch" "4\n"
        |  |
        C  |  # message: C "Branch 2"
        |  |  # author_date: C "2017-09-26 05:55:43-07:00"
        |  |  # modify: C "branch" "2\n"
        |  |
        |  B  # message: B "Branch 1"
        | /   # author_date: B "2017-09-26 05:55:35-07:00"
        |/    # modify: B "branch" "1\n"
        |
        A  # message: A "base"
           # author_date: A "2017-09-26 05:54:56-07:00"
           # modify: A "base" "base\n"
    "#;
}

pub struct BranchUneven;

#[async_trait]
impl TestRepoFixture for BranchUneven {
    const REPO_NAME: &'static str = "branch_uneven";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: L master

        L  # message: L "Add 5"
        |  # author_date: L "2017-09-26 07:20:31-07:00"
        |  # modify: L "5" "5\n"
        |
        K  # message: K "Add 4"
        |  # author_date: K "2017-09-26 07:20:31-07:00"
        |  # modify: K "4" "4\n"
        |
        J  # message: J "Add 3"
        |  # author_date: J "2017-09-26 07:20:31-07:00"
        |  # modify: J "3" "3\n"
        |
        I  # message: I "Add 2"
        |  # author_date: I "2017-09-26 07:20:31-07:00"
        |  # modify: I "2" "2\n"
        |
        H  # message: H "Add 1"
        |  # author_date: H "2017-09-26 07:20:31-07:00"
        |  # modify: H "1" "1\n"
        |
        G  # message: G "Replace the base"
        |  # author_date: G "2017-09-26 07:11:02-07:00"
        |  # modify: G "base" "branch1\n"
        |
        |  F  # message: F "I think 4 is a nice number"
        |  |  # author_date: F "2017-09-26 07:10:41-07:00"
        |  |  # modify: F "branch" "4\n"
        |  |
        |  E  # message: E "Add one"
        |  |  # author_date: E "2017-09-26 05:57:08-07:00"
        |  |  # modify: E "branch" "3\n"
        |  |
        D  |  # message: D "Doubled"
        |  |  # author_date: D "2017-09-26 05:56:52-07:00"
        |  |  # modify: D "branch" "4\n"
        |  |
        C  |  # message: C "Branch 2"
        |  |  # author_date: C "2017-09-26 05:55:43-07:00"
        |  |  # modify: C "branch" "2\n"
        |  |
        |  B  # message: B "Branch 1"
        | /   # author_date: B "2017-09-26 05:55:35-07:00"
        |/    # modify: B "branch" "1\n"
        |
        A  # message: A "base"
        # author_date: A "2017-09-26 05:54:56-07:00"
        # modify: A "base" "base\n"
    "#;
}

pub struct BranchWide;

#[async_trait]
impl TestRepoFixture for BranchWide {
    const REPO_NAME: &'static str = "branch_wide";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: G master

        G  # message: G "Three.four"
        |  # author_date: G "2017-09-27 04:49:05-07:00"
        |  # modify: G "3" "3.4\n"
        |
        |  F  # message: F "Three.three"
        | /   # author_date: F "2017-09-27 04:48:56-07:00"
        |/    # modify: F "3" "3.3\n"
        |
        |  E  # message: E "Three.two"
        |  |  # author_date: E "2017-09-27 04:48:41-07:00"
        |  |  # modify: E "3" "3.2\n"
        |  |
        |  |  D  # message: D "Three.one"
        |  | /   # author_date: D "2017-09-27 04:48:29-07:00"
        |  |/    # modify: D "3" "3.1\n"
        |  |
        C  |  # message: C "Two.two"
        |  |  # author_date: C "2017-09-27 04:29:02-07:00"
        |  |  # modify: C "2" "2.2\n"
        |  |
        |  B  # message: B "Two.one"
        | /   # author_date: B "2017-09-27 04:28:50-07:00"
        |/    # modify: B "2" "2.1\n"
        |
        A  # message: A "One"
           # author_date: A "2017-09-27 04:28:27-07:00"
           # modify: A "1" "1\n"
    "#;
}

pub struct MergeEven;

#[async_trait]
impl TestRepoFixture for MergeEven {
    const REPO_NAME: &'static str = "merge_even";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: H master

        H     # message: H "Merge"
        |\    # author_date: H "2017-09-26 07:13:44-07:00"
        | \   # modify: H "base" "branch1\n"
        |  |  # modify: H "branch" "4\n"
        |  |
        |  G  # message: G "I think 4 is a nice number"
        |  |  # author_date: G "2017-09-26 07:10:41-07:00"
        |  |  # modify: G "branch" "4\n"
        |  |
        |  F  # message: F "Add one"
        |  |  # author_date: F "2017-09-26 05:57:08-07:00"
        |  |  # modify: F "branch" "3\n"
        |  |
        |  E  # message: E "Branch 1"
        |  |  # author_date: E "2017-09-26 05:55:35-07:00"
        |  |  # modify: E "branch" "1\n"
        |  |
        D  |  # message: D "Replace the base"
        |  |  # author_date: D "2017-09-26 07:11:02-07:00"
        |  |  # modify: D "base" "branch1\n"
        |  |
        C  |  # message: C "Doubled"
        |  |  # author_date: C "2017-09-26 05:56:52-07:00"
        |  |  # modify: C "branch" "4\n"
        |  |
        B  |  # message: B "Branch 2"
        | /   # author_date: B "2017-09-26 05:55:43-07:00"
        |/    # modify: B "branch" "2\n"
        |
        A  # message: A "base"
           # author_date: A "2017-09-26 05:54:56-07:00"
           # modify: A "base" "base\n"
    "#;
}

pub struct ManyFilesDirs;

#[async_trait]
impl TestRepoFixture for ManyFilesDirs {
    const REPO_NAME: &'static str = "many_files_dirs";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Stanislau Hlebik <stash@fb.com>"
        # bookmark: A master

        D  # message: D "replace dir1 with a file"
        |  # author_date: D "2018-01-26 02:51:37-08:00"
        |  # modify: D "dir1" "dir1content\n"
        |
        C  # message: C "3"
        |  # author_date: C "2018-01-24 07:36:13-08:00"
        |  # modify: C "dir1/subdir1/subsubdir1/file_1" "content5\n"
        |  # modify: C "dir1/subdir1/subsubdir2/file_1" "content6\n"
        |  # modify: C "dir1/subdir1/subsubdir2/file_2" "content7\n"
        |
        B  # message: B "2"
        |  # author_date: B "2018-01-24 07:34:55-08:00"
        |  # modify: B "2" "2\n"
        |  # modify: B "dir1/file_1_in_dir1" "content1\n"
        |  # modify: B "dir1/file_2_in_dir1" "content3\n"
        |  # modify: B "dir1/subdir1/file_1" "content4\n"
        |  # modify: B "dir2/file_1_in_dir2" "content2\n"
        |
        A  # message: A "1"
           # author_date: A "2018-01-24 07:31:49-08:00"
           # modify: A "1" "1\n"
    "#;
}

pub struct MergeUneven;

#[async_trait]
impl TestRepoFixture for MergeUneven {
    const REPO_NAME: &'static str = "merge_uneven";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: M master

        M     # message: M "Merge two branches"
        |\    # author_date: M "2017-09-26 07:21:12-07:00"
        | \   # modify: M "1" "1\n"
        |  |  # modify: M "2" "2\n"
        |  |  # modify: M "3" "3\n"
        |  |  # modify: M "4" "4\n"
        |  |  # modify: M "5" "5\n"
        |  |  # modify: M "base" "branch1\n"
        |  |  # modify: M "branch" "4\n"
        |  |
        |  L  # message: L "I think 4 is a nice number"
        |  |  # author_date: L "2017-09-26 07:10:41-07:00"
        |  |  # modify: L "branch" "4\n"
        |  |
        |  K  # message: K "Add one"
        |  |  # author_date: K "2017-09-26 05:57:08-07:00"
        |  |  # modify: K "branch" "3\n"
        |  |
        |  J  # message: J "Branch 1"
        |  |  # author_date: J "2017-09-26 05:55:35-07:00"
        |  |  # modify: J "branch" "1\n"
        |  |
        I  |  # message: I "Add 5"
        |  |  # author_date: I "2017-09-26 07:20:31-07:00"
        |  |  # modify: I "5" "5\n"
        |  |
        H  |  # message: H "Add 4"
        |  |  # author_date: H "2017-09-26 07:20:31-07:00"
        |  |  # modify: H "4" "4\n"
        |  |
        G  |  # message: G "Add 3"
        |  |  # author_date: G "2017-09-26 07:20:31-07:00"
        |  |  # modify: G "3" "3\n"
        |  |
        F  |  # message: F "Add 2"
        |  |  # author_date: F "2017-09-26 07:20:31-07:00"
        |  |  # modify: F "2" "2\n"
        |  |
        E  |  # message: E "Add 1"
        |  |  # author_date: E "2017-09-26 07:20:31-07:00"
        |  |  # modify: E "1" "1\n"
        |  |
        D  |  # message: D "Replace the base"
        |  |  # author_date: D "2017-09-26 07:11:02-07:00"
        |  |  # modify: D "base" "branch1\n"
        |  |
        C  |  # message: C "Doubled"
        |  |  # author_date: C "2017-09-26 05:56:52-07:00"
        |  |  # modify: C "branch" "4\n"
        |  |
        B  |  # message: B "Branch 2"
        | /   # author_date: B "2017-09-26 05:55:43-07:00"
        |/    # modify: B "branch" "2\n"
        |
        A  # message: A "base"
        # author_date: A "2017-09-26 05:54:56-07:00"
        # modify: A "base" "base\n"
   "#;
}

pub struct MergeMultipleFiles;

#[async_trait]
impl TestRepoFixture for MergeMultipleFiles {
    const REPO_NAME: &'static str = "merge_multiple_files";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: J master

        J     # message: J "Merge two branches"
        |\    # author_date: J "1974-10-10 06:26:02-07:00"
        | \   # modify: J "1" "1\n"
        |  |  # modify: J "2" "2\n"
        |  |  # modify: J "3" "3\n"
        |  |  # modify: J "4" "4\n"
        |  |  # modify: J "5" "5\n"
        |  |  # modify: J "base" "branch1\n"
        |  |  # modify: J "branch" "4\n"
        |  |
        |  I  # message: I "some other"
        |  |  # author_date: I "2017-09-26 07:11:01-07:00"
        |  |  # modify: I "3" "some other\n"
        |  |
        |  H  # message: H "Other common"
        |  |  # author_date: H "2017-09-26 07:10:51-07:00"
        |  |  # modify: H "base" "other common\n"
        |  |
        |  G  # message: G "I think 4 is a nice number"
        |  |  # author_date: G "2017-09-26 07:10:41-07:00"
        |  |  # modify: G "branch" "4\n"
        |  |
        F  |  # message: F "Add 5"
        |  |  # author_date: F "2017-09-26 07:20:33-07:00"
        |  |  # modify: F "5" "5\n"
        |  |
        E  |  # message: E "Add 3"
        |  |  # author_date: E "2017-09-26 07:20:32-07:00"
        |  |  # modify: E "3" "other\n"
        |  |
        D  |  # message: D "Add 1"
        |  |  # author_date: D "2017-09-26 07:20:31-07:00"
        |  |  # modify: D "1" "1\n"
        |  |
        C  |  # message: C "Replace the base"
        |  |  # author_date: C "2017-09-26 07:11:02-07:00"
        |  |  # modify: C "base" "branch1\n"
        |  |
        B  |  # message: B "Doubled"
        | /   # author_date: B "2017-09-26 05:56:52-07:00"
        |/    # modify: B "branch" "4\n"
        |
        A  # message: A "base"
           # author_date: A "2017-09-26 05:54:56-07:00"
           # modify: A "base" "common\n"
    "#;
}

pub struct UnsharedMergeEven;

#[async_trait]
impl TestRepoFixture for UnsharedMergeEven {
    const REPO_NAME: &'static str = "unshared_merge_even";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: N master

        N  # message: N "And work"
        |  # author_date: N "2017-09-26 09:30:14-07:00"
        |
        M     # message: M "Merge"
        |\    # author_date: M "2017-09-26 09:17:43-07:00"
        | \   # modify: M "side" "merge\n"
        |  |
        L  |  # message: L "Add 5"
        |  |  # author_date: L "2017-09-26 09:17:04-07:00"
        |  |  # modify: L "5" "5\n"
        |  |
        |  K  # message: K "Add 5"
        |  |  # author_date: K "2017-09-26 09:17:09-07:00"
        |  |  # modify: K "5" "5\n"
        |  |
        |  J  # message: J "Add 4"
        |  |  # author_date: J "2017-09-26 09:17:08-07:00"
        |  |  # modify: J "4" "4\n"
        |  |
        |  I  # message: I "Add 3"
        |  |  # author_date: I "2017-09-26 09:17:08-07:00"
        |  |  # modify: I "3" "3\n"
        |  |
        |  H  # message: H "Add 2"
        |  |  # author_date: H "2017-09-26 09:17:08-07:00"
        |  |  # modify: H "2" "2\n"
        |  |
        |  G  # message: G "Add 1"
        |  |  # author_date: G "2017-09-26 09:17:08-07:00"
        |  |  # modify: G "1" "1\n"
        |  |
        F  |  # message: F "Add 4"
        |  |  # author_date: F "2017-09-26 09:17:04-07:00"
        |  |  # modify: F "4" "4\n"
        |  |
        E  |  # message: E "Add 3"
        |  |  # author_date: E "2017-09-26 09:17:04-07:00"
        |  |  # modify: E "3" "3\n"
        |  |
        D  |  # message: D "Add 2"
        |  |  # author_date: D "2017-09-26 09:17:04-07:00"
        |  |  # modify: D "2" "2\n"
        |  |
        C  |  # message: C "Add 1"
        |  |  # author_date: C "2017-09-26 09:17:03-07:00"
        |  |  # modify: C "1" "1\n"
        |  |
        B  |  # message: B "Two"
           |  # author_date: B "2017-09-26 09:02:00-07:00"
           |  # modify: B "side" "2\n"
           |
           A  # message: A "One"
              # author_date: A "2017-09-26 09:01:42-07:00"
              # modify: A "side" "1\n"
    "#;
}

pub struct UnsharedMergeUneven;

#[async_trait]
impl TestRepoFixture for UnsharedMergeUneven {
    const REPO_NAME: &'static str = "unshared_merge_uneven";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "Simon Farnsworth <simonfar@fb.com>"
        # bookmark: S master

        S  # message: S "And remove"
        |  # author_date: S "2017-09-26 09:31:11-07:00"
        |
        R     # message: R "Merge"
        |\    # author_date: R "2017-09-26 09:31:04-07:00"
        | \   # modify: R "10" "10\n"
        |  |  # modify: R "6" "6\n"
        |  |  # modify: R "7" "7\n"
        |  |  # modify: R "8" "8\n"
        |  |  # modify: R "9" "9\n"
        |  |  # modify: R "side" "Merge\n"
        |  |
        |  Q  # message: Q "Add 5"
        |  |  # author_date: Q "2017-09-26 09:17:04-07:00"
        |  |  # modify: Q "5" "5\n"
        |  |
        P  |  # message: P "Add 10"
        |  |  # author_date: P "2017-09-26 09:30:47-07:00"
        |  |  # modify: P "10" "10\n"
        |  |
        O  |  # message: O "Add 9"
        |  |  # author_date: O "2017-09-26 09:30:47-07:00"
        |  |  # modify: O "9" "9\n"
        |  |
        N  |  # message: N "Add 8"
        |  |  # author_date: N "2017-09-26 09:30:46-07:00"
        |  |  # modify: N "8" "8\n"
        |  |
        M  |  # message: M "Add 7"
        |  |  # author_date: M "2017-09-26 09:30:46-07:00"
        |  |  # modify: M "7" "7\n"
        |  |
        L  |  # message: L "Add 6"
        |  |  # author_date: L "2017-09-26 09:30:46-07:00"
        |  |  # modify: L "6" "6\n"
        |  |
        K  |  # message: K "Add 5"
        |  |  # author_date: K "2017-09-26 09:17:09-07:00"
        |  |  # modify: K "5" "5\n"
        |  |
        J  |  # message: J "Add 4"
        |  |  # author_date: J "2017-09-26 09:17:08-07:00"
        |  |  # modify: J "4" "4\n"
        |  |
        I  |  # message: I "Add 3"
        |  |  # author_date: I "2017-09-26 09:17:08-07:00"
        |  |  # modify: I "3" "3\n"
        |  |
        H  |  # message: H "Add 2"
        |  |  # author_date: H "2017-09-26 09:17:08-07:00"
        |  |  # modify: H "2" "2\n"
        |  |
        G  |  # message: G "Add 1"
        |  |  # author_date: G "2017-09-26 09:17:08-07:00"
        |  |  # modify: G "1" "1\n"
        |  |
        |  F  # message: F "Add 4"
        |  |  # author_date: F "2017-09-26 09:17:04-07:00"
        |  |  # modify: F "4" "4\n"
        |  |
        |  E  # message: E "Add 3"
        |  |  # author_date: E "2017-09-26 09:17:04-07:00"
        |  |  # modify: E "3" "3\n"
        |  |
        |  D  # message: D "Add 2"
        |  |  # author_date: D "2017-09-26 09:17:04-07:00"
        |  |  # modify: D "2" "2\n"
        |  |
        |  C  # message: C "Add 1"
        |  |  # author_date: C "2017-09-26 09:17:03-07:00"
        |  |  # modify: C "1" "1\n"
        |  |
        |  B  # message: B "Two"
        |     # author_date: B "2017-09-26 09:02:00-07:00"
        |     # modify: B "side" "2\n"
        |
        A  # message: A "One"
           # author_date: A "2017-09-26 09:01:42-07:00"
           # modify: A "side" "1\n"
    "#;
}

pub async fn save_diamond_commits(
    ctx: &CoreContext,
    repo: &impl Repo,
    parents: Vec<ChangesetId>,
) -> Result<ChangesetId, Error> {
    let first_bcs = create_bonsai_changeset(parents);
    let first_bcs_id = first_bcs.get_changeset_id();

    let second_bcs = create_bonsai_changeset(vec![first_bcs_id]);
    let second_bcs_id = second_bcs.get_changeset_id();

    let third_bcs =
        create_bonsai_changeset_with_author(vec![first_bcs_id], "another_author".to_string());
    let third_bcs_id = third_bcs.get_changeset_id();

    let fourth_bcs = create_bonsai_changeset(vec![second_bcs_id, third_bcs_id]);
    let fourth_bcs_id = fourth_bcs.get_changeset_id();

    changesets_creation::save_changesets(
        ctx,
        repo,
        vec![first_bcs, second_bcs, third_bcs, fourth_bcs],
    )
    .await
    .map(move |()| fourth_bcs_id)
}

pub fn create_bonsai_changeset(parents: Vec<ChangesetId>) -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        message: "message".to_string(),
        ..Default::default()
    }
    .freeze()
    .unwrap()
}

pub fn create_bonsai_changeset_with_author(
    parents: Vec<ChangesetId>,
    author: String,
) -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents,
        author,
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        message: "message".to_string(),
        ..Default::default()
    }
    .freeze()
    .unwrap()
}

pub fn create_bonsai_changeset_with_files(
    parents: Vec<ChangesetId>,
    file_changes: impl Into<SortedVectorMap<NonRootMPath, FileChange>>,
) -> BonsaiChangeset {
    BonsaiChangesetMut {
        parents,
        author: "author".to_string(),
        author_date: DateTime::from_timestamp(0, 0).unwrap(),
        message: "message".to_string(),
        file_changes: file_changes.into(),
        ..Default::default()
    }
    .freeze()
    .unwrap()
}

pub struct ManyDiamonds;

#[async_trait]
impl TestRepoFixture for ManyDiamonds {
    const REPO_NAME: &'static str = "many_diamonds";

    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        let ctx = CoreContext::test_mock(fb);

        fn make_dag(i: u32, prev: Option<(u32, ChangesetId)>) -> String {
            let mut dag = format!(
                r#"
                    # default_files: false
                    # author: * "author"
                    # message: * "message"

                     /--C{i:02}--\         # author: C{i:02} "another_author"
                    A{i:02}       D{i:02}  # bookmark: D{i:02} "master"
                     \--B{i:02}--/
                "#
            );
            if let Some((p, cs_id)) = prev {
                dag.push_str(&format!("\n\nD{p:02}--A{i:02} # exists: D{p:02} {cs_id}"));
            }
            dag
        }

        let (commits, dag) = extend_from_dag_with_actions(&ctx, repo, &make_dag(0, None))
            .await
            .unwrap();
        let mut last_bcs_id = commits["D00"];
        let mut all_commits = commits;
        let mut whole_dag = dag;
        let diamond_stack_size = 50;
        for diamond in 1..diamond_stack_size {
            let (commits, mut dag) = extend_from_dag_with_actions(
                &ctx,
                repo,
                &make_dag(diamond, Some((diamond - 1, last_bcs_id))),
            )
            .await
            .unwrap();
            last_bcs_id = commits[&format!("D{diamond:02}")];
            all_commits.extend(commits);
            dag.extend(whole_dag);
            whole_dag = dag;
        }

        Ok((all_commits, whole_dag))
    }
}

/// A rich fixture spanning multiple nested directories: `top1`, `top2/nested1`,
/// and `top2/nested2`, a repo-root file, an intra-directory copy, a
/// cross-directory copy, a replace-directory-with-a-file, and a merge touching
/// multiple directories. The chain is long enough that `batch_size=3` yields
/// multiple batches. The `top3` tail cycles a top-level stage through every
/// `StageOutput` shape: directory -> file (implicit delete of children) ->
/// directory (file copied under the recreated dir) -> deleted (absent) ->
/// directory again. The `top3` tail also exercises `copy_from` across that
/// dir/file lifecycle: an intra-stage copy within `top3`, a file at the stage
/// root copied from a parent whose stage output is itself a file, a file under
/// `top3` copied from the stage-root path of a parent whose stage output is a
/// file, and a cross-stage copy into `top3`.
pub struct NestedDirectories;

#[async_trait]
impl TestRepoFixture for NestedDirectories {
    const REPO_NAME: &'static str = "nested_directories";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "author"
        # bookmark: Q master

        Q        # message: Q "Cross-stage copy into top3"
        |        # copy: Q "top3/imported" "page1\n" P "top2/nested2/page1"
        |
        P        # message: P "top3 becomes a directory again"
        |        # modify: P "top3/c" "c\n"
        |
        O        # message: O "Delete top3, leaving it absent"
        |        # delete: O "top3"
        |
        R        # message: R "Recreate top3 dir, copying former top3 file into it"
        |        # delete: R "top3"
        |        # copy: R "top3/restored" "copied file\n" N "top3"
        |
        N        # message: N "Copy top3 file from parent's top3 file"
        |        # copy: N "top3" "copied file\n" M "top3"
        |
        M        # message: M "Replace top3 dir with a file (implicit delete of children)"
        |        # modify: M "top3" "now a file\n"
        |
        L        # message: L "Intra-stage copy within top3"
        |        # copy: L "top3/a_copy" "a\n" K "top3/a"
        |
        K        # message: K "Add top3 directory"
        |        # modify: K "top3/a" "a\n"
        |        # modify: K "top3/b" "b\n"
        |
        J        # message: J "Merge side branch, touch top2 and top1"
        |\       # modify: J "top2/nested2/page2" "page2 merged\n"
        | \      # modify: J "top1/lib/util" "util merged\n"
        |  \
        H   I    # message: H "Replace top1/sub dir with a file"
        |   |    # modify: H "top1/sub" "now a file\n"
        |   |    # message: I "Side branch: add top2/nested1/core and root marker"
        |   |    # modify: I "top2/nested1/core" "nested1 core\n"
        |   |    # modify: I "root_marker" "marker\n"
        |  /
        | /
        G        # message: G "Cross-stage copy top2/nested2 -> top1"
        |        # copy: G "top1/copied_from_nested2" "page1\n" F "top2/nested2/page1"
        |
        F        # message: F "Intra-stage copy within top2/nested2"
        |        # copy: F "top2/nested2/page1_copy" "page1\n" E "top2/nested2/page1"
        |
        E        # message: E "Add top2/nested2"
        |        # modify: E "top2/nested2/page1" "page1\n"
        |        # modify: E "top2/nested2/page2" "page2\n"
        |
        D        # message: D "Add top2/nested1"
        |        # modify: D "top2/nested1/helpers" "helpers\n"
        |
        C        # message: C "Add top1/sub dir"
        |        # modify: C "top1/sub/a" "a\n"
        |        # modify: C "top1/sub/b" "b\n"
        |
        B        # message: B "Add top1/lib"
        |        # modify: B "top1/lib/util" "util\n"
        |
        A        # message: A "Root file and top1 file"
                 # modify: A "root_file" "root\n"
                 # modify: A "top1/main" "main\n"
    "#;
}

/// Reproduces the cross-stage copy-source divergence between the canonical and
/// pipelined HgChangeset derivation paths, covering both error branches of
/// `resolve_cross_stage_copy_sources`.
///
/// The tip commit `Q` carries two cross-stage copies, both with DESTINATIONS
/// under `top2` and SOURCES outside `top2` (so both route through
/// `resolve_cross_stage_copy_sources` at the `top2` stage):
///   - `top2/imported` copied from the DIRECTORY `top1` in parent `P` — the
///     source resolves to a TREE, not a file (the not-a-file branch).
///   - `top2/ghost_copy` copied from `no_such_path` in parent `P` — the source
///     does not exist (the not-found branch).
///
/// The canonical path (`resolve_paths` in `derive_hg_changeset.rs`) silently
/// drops both via `into_leaf()?` inside `try_filter_map`, deriving the
/// destination files with no copy metadata, and derivation succeeds. The
/// pipeline path (`resolve_cross_stage_copy_sources`) must match this
/// drop-on-absent / drop-on-non-file behavior to stay byte-identical to
/// canonical.
pub struct CrossStageDirectoryCopy;

#[async_trait]
impl TestRepoFixture for CrossStageDirectoryCopy {
    const REPO_NAME: &'static str = "cross_stage_directory_copy";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "author"
        # bookmark: Q master

        Q        # message: Q "Cross-stage copies of a directory source and a missing source into top2"
        |        # copy: Q "top2/imported" "data\n" P "top1"
        |        # copy: Q "top2/ghost_copy" "data2\n" P "no_such_path"
        |
        P        # message: P "Create top1 directory and a top2 file"
                 # modify: P "top1/main" "main\n"
                 # modify: P "top2/existing" "x\n"
    "#;
}

/// Exercises the SUCCESS path of cross-stage copy-source resolution: a real
/// file copied across a stage boundary, complementing `CrossStageDirectoryCopy`
/// which only covers the drop-on-non-file and drop-on-absent branches.
///
/// Tip commit `Q` copies the real file `top1/main` (in parent `P`, outside the
/// `top2` stage) to `top2/copied`. At the `top2` stage the pipeline must resolve
/// the source filenode from the parent's root manifest, producing output
/// byte-identical to canonical derivation.
pub struct CrossStageFileCopy;

#[async_trait]
impl TestRepoFixture for CrossStageFileCopy {
    const REPO_NAME: &'static str = "cross_stage_file_copy";

    const DAG: &'static str = r#"
        # default_files: false
        # author: * "author"
        # bookmark: Q master

        Q        # message: Q "Cross-stage copy of a real file source into top2"
        |        # copy: Q "top2/copied" "main\n" P "top1/main"
        |
        P        # message: P "Create top1 and top2 files"
                 # modify: P "top1/main" "main\n"
                 # modify: P "top2/existing" "x\n"
    "#;

    // Intentionally skip the default hg-changeset derivation: the pipeline-first
    // harness test relies on canonical hg (`bonsai_hg_mapping`) being absent
    // until the harness derives it, so it can exercise the pipeline-ahead-of-
    // canonical path in `resolve_cross_stage_copy_sources`.
    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        let ctx = CoreContext::test_mock(fb);
        extend_from_dag_with_actions(&ctx, repo, Self::DAG).await
    }
}

/// A nested-directory fixture whose tip commit carries a manifest-altering
/// subtree copy from one top-level directory into another, so it is classified
/// as a `Global` chokepoint. drawdag cannot express subtree changes, so the base
/// graph is built via `extend_from_dag_with_actions` and the chokepoint commit
/// is constructed imperatively.
///
/// The tip commit also modifies a file under the copy SOURCE stage (`top2`) in
/// addition to the subtree copy. This forces the terminal `""` merge to consume
/// the corrupted source-stage intermediate, so the source-stage subtree-copy
/// divergence propagates all the way to the terminal `""` stage (reader-visible
/// at the canonical mapping) instead of being masked by the terminal merge
/// reusing the parent's correct `top2` subtree.
pub struct NestedSubtreeCopy;

#[async_trait]
impl TestRepoFixture for NestedSubtreeCopy {
    const REPO_NAME: &'static str = "nested_subtree_copy";

    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        use std::collections::HashMap;

        use changesets_creation::save_changesets;
        use futures::FutureExt;
        use justknobs::test_helpers::JustKnobsInMemory;
        use justknobs::test_helpers::KnobVal;
        use justknobs::test_helpers::with_just_knobs_async;
        use mononoke_types::MPath;
        use mononoke_types::subtree_change::SubtreeChange;
        use tests_utils::CreateCommitContext;

        let ctx = CoreContext::test_mock(fb);

        let dag = r#"
            # default_files: false
            # author: * "author"

            D    # message: D "Add top2/nested2"
            |    # modify: D "top2/nested2/page" "page\n"
            |
            C    # message: C "Add top2/nested1"
            |    # modify: C "top2/nested1/helpers" "helpers\n"
            |
            B    # message: B "Add top1/lib"
            |    # modify: B "top1/lib/util" "util\n"
            |
            A    # message: A "Add top1/main"
                 # modify: A "top1/main" "main\n"
        "#;
        let (mut commits, mut dag) = extend_from_dag_with_actions(&ctx, repo, dag).await?;

        // Tip commit E: subtree-copy of `top2/` into `top1/from_top2`. The
        // source spans the `top2` stage while the dest is under `top1`, so the
        // barrier is meaningfully cross-stage. Manifest-altering subtree changes
        // require both knobs enabled at save time.
        let parent = commits["D"];
        // Besides the subtree copy, E also modifies a file under the copy SOURCE
        // stage (`top2`). This forces the terminal `""` merge to consume the
        // corrupted source-stage intermediate, propagating the divergence to the
        // reader-visible terminal stage instead of letting the terminal merge
        // reuse the parent's correct `top2` subtree.
        let mut bcs_e = CreateCommitContext::new(&ctx, repo, vec![parent])
            .set_message("E")
            .add_file("top1/marker", "marker\n")
            .add_file("top2/nested1/changed_at_e", "changed at E\n")
            .create_commit_object()
            .await?;
        bcs_e.subtree_changes = vec![(
            MPath::new("top1/from_top2")?,
            SubtreeChange::copy(MPath::new("top2")?, parent),
        )]
        .into_iter()
        .collect();
        let bcs_e = bcs_e.freeze()?;
        let e = bcs_e.get_changeset_id();
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([
                (
                    "scm/mononoke:enable_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
                (
                    "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
            ])),
            async { save_changesets(&ctx, repo, vec![bcs_e]).await }.boxed(),
        )
        .await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.force_set(
            &BookmarkKey::new("master")?,
            e,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.commit().await?;

        commits.insert("E".to_string(), e);
        commits.insert("master".to_string(), e);
        dag.insert("E".to_string(), BTreeSet::from(["D".to_string()]));
        Ok((commits, dag))
    }
}

/// A nested-directory fixture whose tip commit carries a manifest-altering
/// subtree copy whose DEST is a strict ANCESTOR of a deeper pipeline stage.
///
/// Pipeline layout: `top1` is split into a nested `top1/sub` stage, `top2` is a
/// sibling stage. The tip commit copies the whole `top2` subtree onto `top1`
/// (cross-stage, so it is a `Global` chokepoint). The replacement path `top1` is
/// a strict ancestor of the `top1/sub` stage, so that deeper stage's content
/// must become the `sub` sub-slice of the replacement (`top2/sub`), not its
/// stale parent. `top2` deliberately contains its own `sub/` subdirectory with
/// different content so the resolved sub-slice is non-trivial and the divergence
/// is reader-visible. drawdag cannot express subtree changes, so the base graph
/// is built via `extend_from_dag_with_actions` and the tip is built imperatively.
pub struct NestedAncestorSubtreeCopy;

#[async_trait]
impl TestRepoFixture for NestedAncestorSubtreeCopy {
    const REPO_NAME: &'static str = "nested_ancestor_subtree_copy";

    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        use std::collections::HashMap;

        use changesets_creation::save_changesets;
        use futures::FutureExt;
        use justknobs::test_helpers::JustKnobsInMemory;
        use justknobs::test_helpers::KnobVal;
        use justknobs::test_helpers::with_just_knobs_async;
        use mononoke_types::MPath;
        use mononoke_types::subtree_change::SubtreeChange;
        use tests_utils::CreateCommitContext;

        let ctx = CoreContext::test_mock(fb);

        // `top1` has a nested `sub/` directory (its own pipeline stage). `top2`
        // also has a `sub/` directory with different content so the subtree copy
        // meaningfully rewrites the `top1/sub` region.
        let dag = r#"
            # default_files: false
            # author: * "author"

            D    # message: D "Add top2/sub"
            |    # modify: D "top2/sub/a" "a-from-top2\n"
            |    # modify: D "top2/page" "page\n"
            |
            C    # message: C "Add top1/sub"
            |    # modify: C "top1/sub/a" "a-orig\n"
            |
            B    # message: B "Add top1/lib"
            |    # modify: B "top1/lib/util" "util\n"
            |
            A    # message: A "Add top1/main"
                 # modify: A "top1/main" "main\n"
        "#;
        let (mut commits, mut dag) = extend_from_dag_with_actions(&ctx, repo, dag).await?;

        // Tip commit E: subtree-copy of `top2/` onto `top1`. The dest `top1` is a
        // strict ancestor of the `top1/sub` stage, so that deeper stage must
        // reflect `top2/sub` after the copy. E also adds a file under the copy
        // source stage (`top2`) so the divergence is exercised broadly.
        let parent = commits["D"];
        let mut bcs_e = CreateCommitContext::new(&ctx, repo, vec![parent])
            .set_message("E")
            .add_file("top2/marker", "marker\n")
            .create_commit_object()
            .await?;
        bcs_e.subtree_changes = vec![(
            MPath::new("top1")?,
            SubtreeChange::copy(MPath::new("top2")?, parent),
        )]
        .into_iter()
        .collect();
        let bcs_e = bcs_e.freeze()?;
        let e = bcs_e.get_changeset_id();
        with_just_knobs_async(
            JustKnobsInMemory::new(HashMap::from([
                (
                    "scm/mononoke:enable_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
                (
                    "scm/mononoke:enable_manifest_altering_subtree_changes".to_string(),
                    KnobVal::Bool(true),
                ),
            ])),
            async { save_changesets(&ctx, repo, vec![bcs_e]).await }.boxed(),
        )
        .await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.force_set(
            &BookmarkKey::new("master")?,
            e,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.commit().await?;

        commits.insert("E".to_string(), e);
        commits.insert("master".to_string(), e);
        dag.insert("E".to_string(), BTreeSet::from(["D".to_string()]));
        Ok((commits, dag))
    }
}

/// `.slacl` ACL file content (TOML), used by `AclNestedDirectories`.
const ACL_PROJECT1: &[u8] = b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\n";
const ACL_PROJECT2: &[u8] = b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project2\"\n";
const ACL_PROJECT1_WITH_GROUP: &[u8] = b"repo_region_acl = \"REPO_REGION:repos/hg/fbsource/=project1\"\npermission_request_group = \"GROUP:my_group\"\n";

/// A nested-directory fixture exercising the `.slacl` (AclManifest) derivation
/// path: `.slacl` files at the root and several nested levels, interspersed with
/// normal files, plus a `.slacl` modify, delete, implicit-delete, copy carrying
/// a `.slacl`, and a merge touching both top-level dirs. Built imperatively
/// because `.slacl` content is TOML that drawdag cannot express inline. Reused
/// for HgAugmentedManifest (which depends on AclManifest), so keep it general.
pub struct AclNestedDirectories;

#[async_trait]
impl TestRepoFixture for AclNestedDirectories {
    const REPO_NAME: &'static str = "acl_nested_directories";

    async fn init_repo(
        fb: FacebookInit,
        repo: &impl Repo,
    ) -> Result<(
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    )> {
        use tests_utils::CreateCommitContext;

        let ctx = CoreContext::test_mock(fb);
        let mut commits: BTreeMap<String, ChangesetId> = BTreeMap::new();
        let mut dag: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();

        // A: root marker + root `.slacl`; top1 with its own `.slacl`.
        let a = CreateCommitContext::new_root(&ctx, repo)
            .set_message("A")
            .add_file("root_file", "root\n")
            .add_file(".slacl", ACL_PROJECT1)
            .add_file("top1/main", "main\n")
            .add_file("top1/.slacl", ACL_PROJECT2)
            .commit()
            .await?;

        // B: nested `top1/lib/.slacl` (with permission group).
        let b = CreateCommitContext::new(&ctx, repo, vec![a])
            .set_message("B")
            .add_file("top1/lib/util", "util\n")
            .add_file("top1/lib/.slacl", ACL_PROJECT1_WITH_GROUP)
            .commit()
            .await?;

        // C: nested `top1/sub/.slacl`.
        let c = CreateCommitContext::new(&ctx, repo, vec![b])
            .set_message("C")
            .add_file("top1/sub/a", "a\n")
            .add_file("top1/sub/.slacl", ACL_PROJECT2)
            .commit()
            .await?;

        // D: second top-level dir with a nested `.slacl`.
        let d = CreateCommitContext::new(&ctx, repo, vec![c])
            .set_message("D")
            .add_file("top2/nested2/page1", "page1\n")
            .add_file("top2/nested2/.slacl", ACL_PROJECT1)
            .commit()
            .await?;

        // E: MODIFY an existing `.slacl` (top1) and add a normal file.
        let e = CreateCommitContext::new(&ctx, repo, vec![d])
            .set_message("E")
            .add_file("top1/.slacl", ACL_PROJECT1)
            .add_file("top2/normal", "normal\n")
            .commit()
            .await?;

        // F: DELETE a `.slacl` (top1/sub) and a copy that carries a `.slacl`
        // from top2/nested2 into top1/copied.
        let f = CreateCommitContext::new(&ctx, repo, vec![e])
            .set_message("F")
            .delete_file("top1/sub/.slacl")
            .add_file_with_copy_info(
                "top1/copied/.slacl",
                ACL_PROJECT1,
                (e, "top2/nested2/.slacl"),
            )
            .add_file_with_copy_info("top1/copied/page1", "page1\n", (e, "top2/nested2/page1"))
            .commit()
            .await?;

        // G: IMPLICIT delete — replace the `top1/lib` directory (which holds a
        // `.slacl`) with a normal file at the same path. Bonsai semantics
        // implicitly delete the directory's contents.
        let g = CreateCommitContext::new(&ctx, repo, vec![f])
            .set_message("G")
            .add_file("top1/lib", "now a file\n")
            .commit()
            .await?;

        // I: side branch off E adding a `.slacl` under top2/nested1.
        let i = CreateCommitContext::new(&ctx, repo, vec![e])
            .set_message("I")
            .add_file("top2/nested1/core", "core\n")
            .add_file("top2/nested1/.slacl", ACL_PROJECT2)
            .commit()
            .await?;

        // J: MERGE of G and I touching files in both top-level dirs. `top1/lib`
        // is a file in G (implicit delete) but a directory in I (inherited from
        // E), so the merge explicitly resolves it to G's file form.
        let j = CreateCommitContext::new(&ctx, repo, vec![g, i])
            .set_message("J")
            .add_file("top1/lib", "now a file\n")
            .add_file("top1/main", "main merged\n")
            .add_file("top2/nested2/page1", "page1 merged\n")
            .commit()
            .await?;

        let mut txn = repo.bookmarks().create_transaction(ctx.clone());
        txn.force_set(
            &BookmarkKey::new("master")?,
            j,
            BookmarkUpdateReason::TestMove,
        )?;
        txn.commit().await?;

        for (label, cs_id) in [
            ("A", a),
            ("B", b),
            ("C", c),
            ("D", d),
            ("E", e),
            ("F", f),
            ("G", g),
            ("I", i),
            ("J", j),
        ] {
            commits.insert(label.to_string(), cs_id);
        }
        commits.insert("master".to_string(), j);

        dag.insert("A".to_string(), BTreeSet::new());
        dag.insert("B".to_string(), BTreeSet::from(["A".to_string()]));
        dag.insert("C".to_string(), BTreeSet::from(["B".to_string()]));
        dag.insert("D".to_string(), BTreeSet::from(["C".to_string()]));
        dag.insert("E".to_string(), BTreeSet::from(["D".to_string()]));
        dag.insert("F".to_string(), BTreeSet::from(["E".to_string()]));
        dag.insert("G".to_string(), BTreeSet::from(["F".to_string()]));
        dag.insert("I".to_string(), BTreeSet::from(["E".to_string()]));
        dag.insert(
            "J".to_string(),
            BTreeSet::from(["G".to_string(), "I".to_string()]),
        );

        Ok((commits, dag))
    }
}

pub fn json_config_minimal() -> String {
    r#"
    {
        "commit_sync": {
            "test": {
                "large_repo_id": 2100,
                "common_pushrebase_bookmarks": [

                ],
                "small_repos": [

                ],
                "version_name": "test_only"
            }
        }
    }
    "#
    .to_string()
}

pub fn json_config_small() -> String {
    r#"
    {
        "commit_sync": {
            "test": {
                "large_repo_id": 2100,
                "common_pushrebase_bookmarks": [

                ],
                "small_repos": [

                ],
                "version_name": "test_only"
            },
            "xrepo_test_large": {
                "large_repo_id": 504,
                "common_pushrebase_bookmarks": [
                    "master"
                ],
                "small_repos": [
                    {
                        "repoid": 503,
                        "default_action": "prepend_prefix",
                        "default_prefix": "mapping",
                        "bookmark_prefix": "",
                        "mapping": {
                            "README": "README",
                            "something": "mapped/dir1/something"
                        },
                        "direction": "large_to_small"
                    }
                ],
                "version_name": "xrepo_test.v0"
            }
        }
    }
    "#
    .to_string()
}

#[cfg(test)]
mod test {
    use commit_graph::CommitGraphRef;
    use mononoke_macros::mononoke;
    use tests_utils::BasicTestRepo;

    use super::*;

    /// Check that a generated fixture matches the graph from drawdag.
    async fn check_fixture<Fixture: TestRepoFixture + Send>(
        fb: FacebookInit,
        expected_master_hg_id: &str,
    ) {
        let ctx = CoreContext::test_mock(fb);
        let (repo, commits, dag) = Fixture::get_repo_and_dag::<BasicTestRepo>(fb).await;

        // Check all commits in the repo match the graph generated by drawdag.
        assert_eq!(dag.len(), commits.len());
        eprintln!("{dag:?}");
        for (name, parent_names) in dag.iter() {
            let cs_id = commits[name];
            let parents = parent_names
                .iter()
                .map(|name| commits[name])
                .collect::<BTreeSet<_>>();
            let cs_parents = repo
                .commit_graph()
                .changeset_parents(&ctx, cs_id)
                .await
                .unwrap();
            assert_eq!(
                cs_parents.iter().copied().collect::<BTreeSet<_>>(),
                parents,
                "{name} ({cs_id}) parents mismatch: {cs_parents:?} != {parents:?}"
            );
        }

        // Check that master points to a commit with the correct Hg hash.
        let master = repo
            .bookmarks()
            .get(
                ctx.clone(),
                &BookmarkKey::new("master").unwrap(),
                bookmarks::Freshness::MostRecent,
            )
            .await
            .unwrap()
            .expect("master bookmark not found");
        let hg_changeset = repo.derive_hg_changeset(&ctx, master).await.unwrap();
        assert_eq!(hg_changeset.to_hex(), expected_master_hg_id);
    }

    #[mononoke::fbinit_test]
    async fn test_branch_even(fb: FacebookInit) {
        check_fixture::<BranchEven>(fb, "4f7f3fd428bec1a48f9314414b063c706d9c1aed").await;
    }

    #[mononoke::fbinit_test]
    async fn test_branch_uneven(fb: FacebookInit) {
        check_fixture::<BranchUneven>(fb, "264f01429683b3dd8042cb3979e8bf37007118bc").await;
    }

    #[mononoke::fbinit_test]
    async fn test_branch_wide(fb: FacebookInit) {
        check_fixture::<BranchWide>(fb, "49f53ab171171b3180e125b918bd1cf0af7e5449").await;
    }

    #[mononoke::fbinit_test]
    async fn test_many_files_dirs(fb: FacebookInit) {
        check_fixture::<ManyFilesDirs>(fb, "5a28e25f924a5d209b82ce0713d8d83e68982bc8").await;
    }

    #[mononoke::fbinit_test]
    async fn test_linear(fb: FacebookInit) {
        check_fixture::<Linear>(fb, "79a13814c5ce7330173ec04d279bf95ab3f652fb").await;
    }

    #[mononoke::fbinit_test]
    async fn test_merge_even(fb: FacebookInit) {
        check_fixture::<MergeEven>(fb, "1f6bc010883e397abeca773192f3370558ee1320").await;
    }

    #[mononoke::fbinit_test]
    async fn test_merge_uneven(fb: FacebookInit) {
        check_fixture::<MergeUneven>(fb, "d35b1875cdd1ed2c687e86f1604b9d7e989450cb").await;
    }

    #[mononoke::fbinit_test]
    async fn test_merge_multiple_files(fb: FacebookInit) {
        check_fixture::<MergeMultipleFiles>(fb, "c7bfbeed73ed19b01f5309716164d5b37725a61d").await;
    }

    #[mononoke::fbinit_test]
    async fn test_unshared_merge_even(fb: FacebookInit) {
        check_fixture::<UnsharedMergeEven>(fb, "7fe9947f101acb4acf7d945e69f0d6ce76a81113").await;
    }

    #[mononoke::fbinit_test]
    async fn test_unshared_merge_uneven(fb: FacebookInit) {
        check_fixture::<UnsharedMergeUneven>(fb, "dd993aab2bed7276e17c88470286ba8459ba6d94").await;
    }

    #[mononoke::fbinit_test]
    async fn test_many_diamonds(fb: FacebookInit) {
        check_fixture::<ManyDiamonds>(fb, "6b43556e77b7312cabd16ac5f0a85cd920d95272").await;
    }
}
