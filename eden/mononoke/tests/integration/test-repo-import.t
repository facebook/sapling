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
  >      "public_push": true
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

# Run setup checker
  $ cd "$TESTTMP"
  $ echo n | repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  Does the git repo you're about to merge has multiple heads (unmerged branches)? It's unsafe to use this tool when it does. (y/n) * Let's get this merged! (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * Execution error: Failed to fetch common commit sync config: RepositoryId(0) is not a part of any configs (glob)
  Error: Execution failed
  [1]

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

  $ echo n | repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  Does the git repo you're about to merge has multiple heads (unmerged branches)? It's unsafe to use this tool when it does. (y/n) * Let's get this merged! (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * There is no additional setup step needed! (glob)

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
  $ echo n | repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "new_dir/new_repo" \
  > --batch-size 3 \
  > --git-merge-rev-id "$GIT_MASTER_HASH" \
  > --bookmark-suffix "new_repo" \
  > --commit-date-rfc3339 "2005-04-02T21:37:00+01:00" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --commit-author user \
  > --commit-message "merging" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  Does the git repo you're about to merge has multiple heads (unmerged branches)? It's unsafe to use this tool when it does. (y/n) * Let's get this merged! (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Started importing git commits to Mononoke (glob)
  * GitRepo:$TESTTMP/repo-git commit 1 of 5 - Oid:ce435b03 => Bid:ca4f2507 (glob)
  * GitRepo:$TESTTMP/repo-git commit 2 of 5 - Oid:2c01e4a5 => Bid:1ea796ee (glob)
  * GitRepo:$TESTTMP/repo-git commit 3 of 5 - Oid:38f71f7e => Bid:6cfe7a3f (glob)
  * GitRepo:$TESTTMP/repo-git commit 4 of 5 - Oid:13aef6ec => Bid:8a8c80cc (glob)
  * GitRepo:$TESTTMP/repo-git commit 5 of 5 - Oid:6783febd => Bid:3a696e75 (glob)
  * Added commits to Mononoke (glob)
  * Saving gitimported bonsai changesets (glob)
  * Saved gitimported bonsai changesets (glob)
  * Commit 1/5: Remapped ChangesetId(Blake2(ca4f2507b01bd09828c1c223693bf068f3dafc4ac5cdc9f075d4deb4cbae0a27)) => ChangesetId(Blake2(a3050c4a2947243ca6ad1de574a42bc66bf0bb5fd5a6196f486aa9c8f8a3fa63)) (glob)
  * Commit 2/5: Remapped ChangesetId(Blake2(1ea796eecfc63927036609b8d932d7fa0531d69347032539ef5ae16d6bb700bb)) => ChangesetId(Blake2(f368b430b2d6274f8735369dd5a377ef266c8e64772d3007ba4fd753a949daee)) (glob)
  * Commit 3/5: Remapped ChangesetId(Blake2(6cfe7a3ff0e43160d780c429ed7aa5b825c956c0076998f5442d520a97bbd904)) => ChangesetId(Blake2(6efb2b7334fc122a014fc488379e8f8698c08edc91302bc278efd3221a84a3bb)) (glob)
  * Commit 4/5: Remapped ChangesetId(Blake2(8a8c80cc37859f8fe01d9500793f7c8066684581c0f6161de46798f30e207c1b)) => ChangesetId(Blake2(b17e527da034a365ca779eddba9d19cf50982114ee6e8e27b6ca94b0706d7e7e)) (glob)
  * Commit 5/5: Remapped ChangesetId(Blake2(3a696e759cca23266e23beb6ee2f367aca1241d7d9e52a6ee9733dea79a98955)) => ChangesetId(Blake2(a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca)) (glob)
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
  * flushing 2 in-memory IdMap entries to SQL (glob)
  * IdMap updated, IdDag updated (glob)
  * segmented changelog version saved, idmap_version: 1, iddag_version: * (glob)
  * successful incremental update to segmented changelog (glob)
  * SegmentedChangelogTailer is done (glob)
  * Finished tailing segmented changelog (glob)
  * Start moving the bookmark (glob)
  * Created bookmark BookmarkName { bookmark: "repo_import_new_repo" } pointing to a3050c4a2947243ca6ad1de574a42bc66bf0bb5fd5a6196f486aa9c8f8a3fa63 (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to point to ChangesetId(Blake2(6efb2b7334fc122a014fc488379e8f8698c08edc91302bc278efd3221a84a3bb)) (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to point to ChangesetId(Blake2(a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca)) (glob)
  * Finished moving the bookmark (glob)
  * Merging the imported commits into given bookmark, master_bookmark (glob)
  * Done checking path conflicts (glob)
  * Creating a merge bonsai changeset with parents: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd, f368b430b2d6274f8735369dd5a377ef266c8e64772d3007ba4fd753a949daee (glob)
  * Created merge bonsai: 290846903d9c4be3ae21482497f0a346e525577f8697833d987b541f5b027154 and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), ChangesetId(Blake2(f368b430b2d6274f8735369dd5a377ef266c8e64772d3007ba4fd753a949daee))], author: "user", author_date: DateTime(2005-04-02T21:37:00+01:00), committer: Some("user"), committer_date: Some(DateTime(2005-04-02T21:37:00+01:00)), message: "merging", extra: {}, file_changes: {}, is_snapshot: false }, id: ChangesetId(Blake2(290846903d9c4be3ae21482497f0a346e525577f8697833d987b541f5b027154)) } (glob)
  * Finished merging (glob)
  * Running pushrebase (glob)
  * Finished pushrebasing to 290846903d9c4be3ae21482497f0a346e525577f8697833d987b541f5b027154 (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to the merge commit: ChangesetId(Blake2(290846903d9c4be3ae21482497f0a346e525577f8697833d987b541f5b027154)) (glob)

# Check if we derived all the types for imported commits. Checking last one after bookmark move, before setting it to the merge commit.
  $ MERGE_PARENT_GIT="a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca"
  $ mononoke_admin derived-data exists changeset_info $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists blame $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists deleted_manifest $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists fastlog $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists filenodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists fsnodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists hgchangesets $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca
  $ mononoke_admin derived-data exists unodes $MERGE_PARENT_GIT 2> /dev/null
  Derived: a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca

# Start Mononoke
  $ start_and_wait_for_mononoke_server
# Clone the repository
  $ cd "$TESTTMP"
  $ jq -S '.' "$GIT_REPO/recovery_file.json" > "$GIT_REPO/recovery_file_sorted.json"
  $ cat "$GIT_REPO/recovery_file_sorted.json"
  {
    "batch_size": 3,
    "bookmark_suffix": "new_repo",
    "commit_author": "user",
    "commit_message": "merging",
    "datetime": * (glob)
    "dest_bookmark_name": "master_bookmark",
    "dest_path": "new_dir/new_repo",
    "git_merge_bcs_id": "1ea796eecfc63927036609b8d932d7fa0531d69347032539ef5ae16d6bb700bb",
    "git_merge_rev_id": "2c01e4a5658421e2bfcd08e31d9b69399319bcd3",
    "git_repo_path": "$TESTTMP/repo-git",
    "gitimport_bcs_ids": [
      "ca4f2507b01bd09828c1c223693bf068f3dafc4ac5cdc9f075d4deb4cbae0a27",
      "1ea796eecfc63927036609b8d932d7fa0531d69347032539ef5ae16d6bb700bb",
      "6cfe7a3ff0e43160d780c429ed7aa5b825c956c0076998f5442d520a97bbd904",
      "8a8c80cc37859f8fe01d9500793f7c8066684581c0f6161de46798f30e207c1b",
      "3a696e759cca23266e23beb6ee2f367aca1241d7d9e52a6ee9733dea79a98955"
    ],
    "hg_sync_check_disabled": true,
    "import_stage": "PushCommit",
    "imported_cs_id": "f368b430b2d6274f8735369dd5a377ef266c8e64772d3007ba4fd753a949daee",
    "merged_cs_id": * (glob)
    "move_bookmark_commits_done": 4,
    "phab_check_disabled": true,
    "recovery_file_path": "$TESTTMP/repo-git/recovery_file.json",
    "shifted_bcs_ids": [
      "a3050c4a2947243ca6ad1de574a42bc66bf0bb5fd5a6196f486aa9c8f8a3fa63",
      "f368b430b2d6274f8735369dd5a377ef266c8e64772d3007ba4fd753a949daee",
      "6efb2b7334fc122a014fc488379e8f8698c08edc91302bc278efd3221a84a3bb",
      "b17e527da034a365ca779eddba9d19cf50982114ee6e8e27b6ca94b0706d7e7e",
      "a1f9e1fa71d785bca63ee3f70b89ac8f26abee8b13817045ff8f7f2e875e8fca"
    ],
    "sleep_time": {
      "nanos": 0,
      "secs": 5
    },
    "x_repo_check_disabled": false
  }

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo1 --noupdate -q
  $ cd repo1
  $ hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
  adding remote bookmark repo_import_new_repo
  $ hgmn up master_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

  $ log -r "ancestors(master_bookmark)"
  @    merging [draft;rev=5;b1d548bf0199]
  ├─╮
  │ o  Add file3 [draft;rev=4;7365931b3a41]
  │ │
  │ o  Add file1 and file2 [draft;rev=3;bcc429b3666b]
  │
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $

  $ ls
  A
  B
  C
  new_dir

  $ cat "new_dir/new_repo/file1"
  this is file1
  $ cat "new_dir/new_repo/file2_repo/file2"
  this is file2
  $ cat "new_dir/new_repo/file3_repo/file3"
  this is file3
