# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

# Test that repo_import ensures that when importing into a large repo, the mapping
# provided exists and is indeed a large-repo only mapping.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

Setup configuration
  $ setup_configerator_configs
-- Init Mononoke thingies
  $ init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

-- Setup git repository
  $ cd "$TESTTMP"
  $ export GIT_REPO=git_repo
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "file1" > file1
  $ git add file1
  $ git commit -aqm "Add file1"
  $ echo "file1a" > file1
  $ git commit -aqm "Modify file1 A"
  $ echo "file1b" > file1
  $ git commit -aqm "Modify file1 B"

-- Try to import passing a mapping that does not exist
-- SHOULD FAIL
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDLARGE"
  $ with_stripped_logs repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "imported" \
  > --batch-size 3 \
  > --bookmark-suffix "imported" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master_bookmark \
  > --commit-author user \
  > --commit-message "merging" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "non_existent_version" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  using repo "large-mon" repoid RepositoryId(0)
  Started importing git commits to Mononoke
  GitRepo:git_repo commit 3 of 3 - Oid:65d77e43 => Bid:8f2b6318, repo: git_repo
  Added commits to Mononoke
  Commit 1/3: Remapped ChangesetId(Blake2(a7757103990501ed893e42654879d163d8954a664bfd9ff2374a3d49ba5b4def)) => ChangesetId(Blake2(ee8403dabe5d6dd4bff1f7710bce9911e9652d401b28d93f46d17ca7b750a6ff))
  Commit 2/3: Remapped ChangesetId(Blake2(eecbdd3e303e139655d098530a5b9bc8c510c82dd6bc4d82d985010b23e50c74)) => ChangesetId(Blake2(cd3a7e1c74d34718522c7fd1218d5c25f252bf5e53418d5f31f2c75ffca6e008))
  Commit 3/3: Remapped ChangesetId(Blake2(8f2b6318e945f2a94d7510b90c2ea4b2f56db0010f52a88d1004aa25021cf380)) => ChangesetId(Blake2(6f3d4345ba964c6b92f2379e989878b77f0215b89e58eaaa5e5949f9d1ffcabb))
  Saving shifted bonsai changesets
  Saved shifted bonsai changesets
  Start deriving data types
  Finished deriving data types
  Start moving the bookmark
  Created bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_imported" }, category: Branch } pointing to ee8403dabe5d6dd4bff1f7710bce9911e9652d401b28d93f46d17ca7b750a6ff
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_imported" }, category: Branch } to point to ChangesetId(Blake2(6f3d4345ba964c6b92f2379e989878b77f0215b89e58eaaa5e5949f9d1ffcabb))
  Finished moving the bookmark
  Merging the imported commits into given bookmark, master_bookmark
  Done checking path conflicts
  Creating a merge bonsai changeset with parents: 3e020372209167db53084d8295a9d94bb1cd654e19711da331d5b05c0467f9a0, 6f3d4345ba964c6b92f2379e989878b77f0215b89e58eaaa5e5949f9d1ffcabb
  Created merge bonsai: e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8 and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(3e020372209167db53084d8295a9d94bb1cd654e19711da331d5b05c0467f9a0)), ChangesetId(Blake2(6f3d4345ba964c6b92f2379e989878b77f0215b89e58eaaa5e5949f9d1ffcabb))], author: "user", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: Some("user"), committer_date: Some(DateTime(1970-01-01T00:00:00+00:00)), message: "merging", hg_extra: {}, git_extra_headers: None, file_changes: {}, is_snapshot: false, git_tree_hash: None, git_annotated_tag: None }, id: ChangesetId(Blake2(e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8)) }
  Finished merging
  Running pushrebase
  Finished pushrebasing to e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_imported" }, category: Branch } to the merge commit: ChangesetId(Blake2(e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8))


-- Try to import passing a mapping that is not large-repo only
-- SHOULD FAIL
  $ with_stripped_logs repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "imported2" \
  > --batch-size 3 \
  > --bookmark-suffix "imported" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master_bookmark \
  > --commit-author user \
  > --commit-message "merging again" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "test_version" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  using repo "large-mon" repoid RepositoryId(0)
  Started importing git commits to Mononoke
  GitRepo:git_repo 3 of 3 commit(s) already exist, repo: git_repo
  Added commits to Mononoke
  Commit 1/3: Remapped ChangesetId(Blake2(a7757103990501ed893e42654879d163d8954a664bfd9ff2374a3d49ba5b4def)) => ChangesetId(Blake2(25bcc60f08e128a2d4d2c7c162d835bdfaf0d196e90151ded7f26e89728060c8))
  Commit 2/3: Remapped ChangesetId(Blake2(eecbdd3e303e139655d098530a5b9bc8c510c82dd6bc4d82d985010b23e50c74)) => ChangesetId(Blake2(5f245656f10638629b94c591c490a59049093aff1322dca45058c93f430669d9))
  Commit 3/3: Remapped ChangesetId(Blake2(8f2b6318e945f2a94d7510b90c2ea4b2f56db0010f52a88d1004aa25021cf380)) => ChangesetId(Blake2(143a9a7513c50ae6cf232654dd9cf996d98b5787c392bad353c55b2bb7cc89a9))
  Saving shifted bonsai changesets
  Saved shifted bonsai changesets
  Start deriving data types
  Finished deriving data types
  Start moving the bookmark
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_imported" }, category: Branch } to point to ChangesetId(Blake2(143a9a7513c50ae6cf232654dd9cf996d98b5787c392bad353c55b2bb7cc89a9))
  Finished moving the bookmark
  Merging the imported commits into given bookmark, master_bookmark
  Done checking path conflicts
  Creating a merge bonsai changeset with parents: e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8, 143a9a7513c50ae6cf232654dd9cf996d98b5787c392bad353c55b2bb7cc89a9
  Created merge bonsai: 6c968da2c94228839ece4bdf6adf826e20cfbe9ccd07fc9a2bd67a934f097ee8 and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8)), ChangesetId(Blake2(143a9a7513c50ae6cf232654dd9cf996d98b5787c392bad353c55b2bb7cc89a9))], author: "user", author_date: DateTime(1970-01-01T00:00:00+00:00), committer: Some("user"), committer_date: Some(DateTime(1970-01-01T00:00:00+00:00)), message: "merging again", hg_extra: {}, git_extra_headers: None, file_changes: {}, is_snapshot: false, git_tree_hash: None, git_annotated_tag: None }, id: ChangesetId(Blake2(6c968da2c94228839ece4bdf6adf826e20cfbe9ccd07fc9a2bd67a934f097ee8)) }
  Finished merging
  Running pushrebase
  Finished pushrebasing to 6c968da2c94228839ece4bdf6adf826e20cfbe9ccd07fc9a2bd67a934f097ee8
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_imported" }, category: Branch } to the merge commit: ChangesetId(Blake2(6c968da2c94228839ece4bdf6adf826e20cfbe9ccd07fc9a2bd67a934f097ee8))
