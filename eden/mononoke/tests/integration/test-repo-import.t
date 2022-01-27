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

# Run setup checker
  $ cd "$TESTTMP"
  $ repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
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

  $ repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * There is no additional setup step needed! (glob)

# run segmented changelog tailer on master bookmark
  $ cat >> "$TESTTMP/mononoke-config/repos/repo/server.toml" <<CONFIG
  > [segmented_changelog_config]
  > master_bookmark="master_bookmark"
  > CONFIG
  $ segmented_changelog_tailer_reseed --repo repo  2>&1 | grep -e successfully -e segmented_changelog_tailer
  * repo name 'repo' translates to id 0 (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * repo 0: successfully seeded segmented changelog (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)

# Import the repo
# Segmented changelog should be rebuild for newly imported commits along the way.
  $ repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "new_dir/new_repo" \
  > --batch-size 3 \
  > --bookmark-suffix "new_repo" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --commit-author user \
  > --commit-message "merging" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  * using repo "repo" repoid RepositoryId(0) (glob)
  *Reloading redacted config from configerator* (glob)
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * Started importing git commits to Mononoke (glob)
  * GitRepo:* commit 1 of 2 - Oid:* => Bid:* (glob)
  * GitRepo:* commit 2 of 2 - Oid:* => Bid:* (glob)
  * Added commits to Mononoke (glob)
  * Saving gitimported bonsai changesets (glob)
  * Saved gitimported bonsai changesets (glob)
  * Remapped ChangesetId(Blake2(*)) => ChangesetId(Blake2(*)) (glob)
  * Remapped ChangesetId(Blake2(*)) => ChangesetId(Blake2(*)) (glob)
  * Saving shifted bonsai changesets (glob)
  * Saved shifted bonsai changesets (glob)
  * Start deriving data types (glob)
  * Finished deriving data types (glob)
  * Start tailing segmented changelog (glob)
  * using 'Bookmark master_bookmark' for head (glob)
  * repo 0: SegmentedChangelogTailer initialized (glob)
  * repo 0: starting incremental update to segmented changelog (glob)
  * Adding hints for repo 0 idmap_version 1 (glob)
  * repo 0 idmap_version 1 has a full set of hints * (glob)
  * repo 0: IdMap updated, IdDag updated (glob)
  * repo 0: segmented changelog version saved, idmap_version: 1, iddag_version: * (glob)
  * repo 0: successful incremental update to segmented changelog (glob)
  * repo 0: SegmentedChangelogTailer is done (glob)
  * Finished tailing segmented changelog (glob)
  * Start moving the bookmark (glob)
  * Created bookmark BookmarkName { bookmark: "repo_import_new_repo" } pointing to * (glob)
  * Set bookmark BookmarkName { bookmark: "repo_import_new_repo" } to * (glob)
  * Finished moving the bookmark (glob)
  * Merging the imported commits into given bookmark, master_bookmark (glob)
  * Done checking path conflicts (glob)
  * Creating a merge bonsai changeset with parents: *, * (glob)
  * Created merge bonsai: * and changeset: * (glob)
  * Finished merging (glob)
  * Running pushrebase (glob)
  * Finished pushrebasing to * (glob)

# Check if we derived all the types
  $ BOOKMARK_NAME="repo_import_new_repo"
  $ mononoke_admin derived-data exists changeset_info $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists blame $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists deleted_manifest $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists fastlog $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists filenodes $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists fsnodes $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists hgchangesets $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681
  $ mononoke_admin derived-data exists unodes $BOOKMARK_NAME 2> /dev/null
  Derived: 2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681

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
    "git_repo_path": "$TESTTMP/repo-git",
    "gitimport_bcs_ids": [
      "*", (glob)
      "*" (glob)
    ],
    "hg_sync_check_disabled": true,
    "import_stage": "PushCommit",
    "imported_cs_id": "2dcfd5aae7492591bca9870e9679b74ca607f50093a667c635b3e3e183c11681",
    "merged_cs_id": * (glob)
    "move_bookmark_commits_done": 1,
    "phab_check_disabled": true,
    "recovery_file_path": "$TESTTMP/repo-git/recovery_file.json",
    "shifted_bcs_ids": [
      "*", (glob)
      "*" (glob)
    ],
    "sleep_time": 5,
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

  $ log -r "all()"
  @    merging [draft;rev=5;*] (glob)
  ├─╮
  │ o  Add file3 [draft;rev=4;*] (glob)
  │ │
  │ o  Add file1 and file2 [draft;rev=3;*] (glob)
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
