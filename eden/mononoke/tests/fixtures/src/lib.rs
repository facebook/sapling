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
use tests_utils::BasicTestRepo;
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

    async fn get_test_repo_and_dag(
        fb: FacebookInit,
    ) -> (
        BasicTestRepo,
        BTreeMap<String, ChangesetId>,
        BTreeMap<String, BTreeSet<String>>,
    ) {
        let repo: BasicTestRepo = TestRepoFactory::new(fb)
            .unwrap()
            .with_id(RepositoryId::new(0))
            .with_name(Self::REPO_NAME.to_string())
            .build()
            .await
            .unwrap();
        let (commits, dag) = Self::init_repo(fb, &repo).await.unwrap();
        (repo, commits, dag)
    }

    async fn get_test_repo(fb: FacebookInit) -> BasicTestRepo {
        let (repo, _, _) = Self::get_test_repo_and_dag(fb).await;
        repo
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
            all_commits.extend(commits.into_iter());
            dag.extend(whole_dag.into_iter());
            whole_dag = dag;
        }

        Ok((all_commits, whole_dag))
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

    use super::*;

    /// Check that a generated fixture matches the graph from drawdag.
    async fn check_fixture<Fixture: TestRepoFixture + Send>(
        fb: FacebookInit,
        expected_master_hg_id: &str,
    ) {
        let ctx = CoreContext::test_mock(fb);
        let (repo, commits, dag) = Fixture::get_test_repo_and_dag(fb).await;

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
                "{name} ({cs_id}) parents mismatch: {:?} != {:?}",
                cs_parents,
                parents
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
