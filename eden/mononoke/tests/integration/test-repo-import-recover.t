# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ HG_REPO="${TESTTMP}/repo-hg"
  $ BLOB_TYPE="blob_files" default_setup
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting
  starting Mononoke
  cloning repo in hg client 'repo2'
  $ SKIP_CROSS_REPO_CONFIG=1 setup_configerator_configs
  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "0": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF

# Setup git repository
  $ mkdir "$GIT_REPO"
  $ cd "$GIT_REPO"
  $ git init -q
  $ echo "this is file1" > file1
  $ mkdir file2_repo
  $ cd file2_repo
  $ echo "this is file2" > file2
  $ cd ..
  $ git add file1 file2_repo/file2
  $ git commit -am "Add file1 and file2"
  [master (root-commit) ce435b0] Add file1 and file2
   2 files changed, 2 insertions(+)
   create mode 100644 file1
   create mode 100644 file2_repo/file2
  $ mkdir file3_repo
  $ echo "this is file3" > file3_repo/file3
  $ git add file3_repo/file3
  $ git commit -am "Add file3"
  [master 2c01e4a] Add file3
   1 file changed, 1 insertion(+)
   create mode 100644 file3_repo/file3
  $ git checkout -b other_branch
  Switched to a new branch 'other_branch'
  $ for i in {0..2}; do echo $i > file.$i; git add .; git commit -m "commit $i"; done >/dev/null
  $ git log --graph --oneline --all --decorate
  * 6783feb (HEAD -> other_branch) commit 2
  * 13aef6e commit 1
  * 38f71f7 commit 0
  * 2c01e4a (master) Add file3
  * ce435b0 Add file1 and file2
  $ GIT_MASTER_HASH=$(git log -n 1 --pretty=format:"%H" master)


# run segmented changelog tailer on master bookmark
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > heads_to_include = [
  >    { bookmark = "master_bookmark" },
  > ]
  > CONFIG
  $ segmented_changelog_tailer_reseed --repo repo  2>&1 | grep -e successfully -e segmented_changelog_tailer
  * repo name 'repo' translates to id 0 (glob)
  * SegmentedChangelogTailer initialized, repo_id: 0 (glob)
  * successfully seeded segmented changelog, repo_id: 0 (glob)
  * SegmentedChangelogTailer is done, repo_id: 0 (glob)

# Import the repo
# Segmented changelog should be rebuild for newly imported commits along the way.
  $ cd "$TESTTMP"
  $ cat > "${TESTTMP}/recovery_file.json" <<EOF
  >  {
  >    "batch_size": 3,
  >    "bookmark_suffix": "new_repo",
  >    "commit_author": "user",
  >    "commit_message": "merging",
  >    "datetime": "2005-04-02T21:37:00+01:00",
  >    "dest_bookmark_name": "master_bookmark",
  >    "dest_path": "new_dir/new_repo",
  >    "git_merge_bcs_id": null,
  >    "git_merge_rev_id": "2c01e4a5658421e2bfcd08e31d9b69399319bcd3",
  >    "git_repo_path": "${TESTTMP}/repo-git",
  >    "gitimport_bcs_ids": null, 
  >    "hg_sync_check_disabled": true,
  >    "import_stage": "GitImport",
  >    "imported_cs_id": null,
  >    "merged_cs_id": null,
  >    "move_bookmark_commits_done": 0,
  >    "phab_check_disabled": true,
  >    "recovery_file_path": "${TESTTMP}/recovery_file.json",
  >    "shifted_bcs_ids": null,
  >    "sleep_time": {
  >      "nanos": 0,
  >      "secs": 5
  >    },
  >    "x_repo_check_disabled": false
  >  }
  > EOF
  $ echo n | repo_import \
  >  recover-process \
  > "$TESTTMP/recovery_file.json"
  Does the git repo you're about to merge has multiple heads (unmerged branches)? It's unsafe to use this tool when it does. (y/n) * Let's get this merged! (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Fetching the recovery stage for importing (glob)
  * Fetched the recovery stage for importing. (glob)
  Starting from stage: GitImport
  * Started importing git commits to Mononoke (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 5 - Oid:ce435b03 => Bid:071d73e6 (glob)
  * GitRepo:$TESTTMP/repo-git commit 2 of 5 - Oid:2c01e4a5 => Bid:4dbc9506 (glob)
  * GitRepo:$TESTTMP/repo-git commit 3 of 5 - Oid:38f71f7e => Bid:d805ae48 (glob)
  * GitRepo:$TESTTMP/repo-git commit 4 of 5 - Oid:13aef6ec => Bid:260f78ba (glob)
  * GitRepo:$TESTTMP/repo-git commit 5 of 5 - Oid:6783febd => Bid:8d76deb1 (glob)
  * Added commits to Mononoke (glob)
  * Commit 1/5: Remapped ChangesetId(Blake2(071d73e6b97823ffbde324c6147a785013f479157ade3f83c9b016c8f40c09de)) => ChangesetId(Blake2(4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a)) (glob)
  * Commit 2/5: Remapped ChangesetId(Blake2(4dbc950685a833a9329f7f31116b92232f6d759769c699ded44fba4e239c66a4)) => ChangesetId(Blake2(fea472cdf364ad6499f20e5f32c0ba01cb73fda8cab229c24f456df085b17622)) (glob)
  * Commit 3/5: Remapped ChangesetId(Blake2(d805ae48f71b290203959f8b9eb859bea762989fe5c32439dbd39f48c9050960)) => ChangesetId(Blake2(6b49fda25c209960aad992721e872237737671564a6ce0f0347f04f4c0fee177)) (glob)
  * Commit 4/5: Remapped ChangesetId(Blake2(260f78ba75e428610060f950dc7b4aa06a81e8b34179a38e6f46492f90c76084)) => ChangesetId(Blake2(5d2a4db5b6b759b8767ed501d1a53a4bec89ea3778bfa9516b62c6986c78f132)) (glob)
  * Commit 5/5: Remapped ChangesetId(Blake2(8d76deb176f7a48e0ab67b66cb791c6461406b6e35aedc440f6e4f9e3b27127c)) => ChangesetId(Blake2(11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9)) (glob)
  * Saving shifted bonsai changesets (glob)
  * Saved shifted bonsai changesets (glob)
  * Start deriving data types (glob)
  * Finished deriving data types (glob)
  * Start tailing segmented changelog (glob)
  * Using the following segmented changelog heads: [Bookmark(BookmarkName { bookmark: "master_bookmark" })] (glob)
  * SegmentedChangelogTailer initialized (glob)
  * starting incremental update to segmented changelog (glob)
  * iddag initialized, it covers 3 ids (glob)
  * starting the actual update (glob)
  * Adding hints for idmap_version 1 (glob)
  * idmap_version 1 has a full set of hints * (glob)
  * IdMap updated, IdDag updated (glob)
  * segmented changelog version saved, idmap_version: 1, iddag_version: * (glob)
  * successful incremental update to segmented changelog (glob)
  * SegmentedChangelogTailer is done (glob)
  * Finished tailing segmented changelog (glob)
  * Start moving the bookmark (glob)
  * Created bookmark BookmarkName { bookmark: "repo_import_new_repo" } pointing to 4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to point to ChangesetId(Blake2(6b49fda25c209960aad992721e872237737671564a6ce0f0347f04f4c0fee177)) (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to point to ChangesetId(Blake2(11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9)) (glob)
  * Finished moving the bookmark (glob)
  * Merging the imported commits into given bookmark, master_bookmark (glob)
  * Done checking path conflicts (glob)
  * Creating a merge bonsai changeset with parents: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd, fea472cdf364ad6499f20e5f32c0ba01cb73fda8cab229c24f456df085b17622 (glob)
  * Created merge bonsai: 3b27309e57b5809695eb8978fa999d34fafbbe33d17420d9e0301351a19e40a1 and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), ChangesetId(Blake2(fea472cdf364ad6499f20e5f32c0ba01cb73fda8cab229c24f456df085b17622))], author: "user", author_date: DateTime(2005-04-02T21:37:00+01:00), committer: Some("user"), committer_date: Some(DateTime(2005-04-02T21:37:00+01:00)), message: "merging", extra: {}, file_changes: {}, is_snapshot: false }, id: ChangesetId(Blake2(3b27309e57b5809695eb8978fa999d34fafbbe33d17420d9e0301351a19e40a1)) } (glob)
  * Finished merging (glob)
  * Running pushrebase (glob)
  * Finished pushrebasing to 3b27309e57b5809695eb8978fa999d34fafbbe33d17420d9e0301351a19e40a1 (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to the merge commit: ChangesetId(Blake2(3b27309e57b5809695eb8978fa999d34fafbbe33d17420d9e0301351a19e40a1)) (glob)

# Check if we derived all the types for imported commits. Checking last one after bookmark move, before setting it to the merge commit.
  $ MERGE_PARENT_GIT="11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9"
  $ mononoke_admin derived-data exists changeset_info $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists blame $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists deleted_manifest $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists fastlog $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists filenodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists fsnodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists hgchangesets $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9
  $ mononoke_admin derived-data exists unodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: 11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9

# Check the recovery file
  $ cd "$TESTTMP"
  $ jq -S '.' "$TESTTMP/recovery_file.json" > "$TESTTMP/recovery_file_sorted.json"
  $ cat "$TESTTMP/recovery_file_sorted.json"
  {
    "batch_size": 3,
    "bookmark_suffix": "new_repo",
    "commit_author": "user",
    "commit_message": "merging",
    "datetime": * (glob)
    "dest_bookmark_name": "master_bookmark",
    "dest_path": "new_dir/new_repo",
    "git_merge_bcs_id": "4dbc950685a833a9329f7f31116b92232f6d759769c699ded44fba4e239c66a4",
    "git_merge_rev_id": "2c01e4a5658421e2bfcd08e31d9b69399319bcd3",
    "git_repo_path": "$TESTTMP/repo-git",
    "gitimport_bcs_ids": [
      "071d73e6b97823ffbde324c6147a785013f479157ade3f83c9b016c8f40c09de",
      "4dbc950685a833a9329f7f31116b92232f6d759769c699ded44fba4e239c66a4",
      "d805ae48f71b290203959f8b9eb859bea762989fe5c32439dbd39f48c9050960",
      "260f78ba75e428610060f950dc7b4aa06a81e8b34179a38e6f46492f90c76084",
      "8d76deb176f7a48e0ab67b66cb791c6461406b6e35aedc440f6e4f9e3b27127c"
    ],
    "hg_sync_check_disabled": true,
    "import_stage": "PushCommit",
    "imported_cs_id": "fea472cdf364ad6499f20e5f32c0ba01cb73fda8cab229c24f456df085b17622",
    "merged_cs_id": * (glob)
    "move_bookmark_commits_done": 4,
    "phab_check_disabled": true,
    "recovery_file_path": "$TESTTMP/recovery_file.json",
    "shifted_bcs_ids": [
      "4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a",
      "fea472cdf364ad6499f20e5f32c0ba01cb73fda8cab229c24f456df085b17622",
      "6b49fda25c209960aad992721e872237737671564a6ce0f0347f04f4c0fee177",
      "5d2a4db5b6b759b8767ed501d1a53a4bec89ea3778bfa9516b62c6986c78f132",
      "11b1e6976133cca327762371e8c523d3a0cd3ff2abe34385c8253df72cc989a9"
    ],
    "sleep_time": {
      "nanos": 0,
      "secs": 5
    },
    "x_repo_check_disabled": false
  }
