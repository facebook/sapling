# Copyright (c) Facebook, Inc. and its affiliates.
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
  $ setup_configerator_configs
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
  $ git init
  Initialized empty Git repository in $TESTTMP/repo-git/.git/
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
  > --local-configerator-path="$TESTTMP/configerator" \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * using repo "repo" repoid RepositoryId(0) (glob)
  * Execution error: The repo (repo) doesn't have a commit sync config (glob)
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
  > --local-configerator-path="$TESTTMP/configerator" \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * There is no additional setup step needed! (glob)

# Import the repo
  $ repo_import \
  > --local-configerator-path="$TESTTMP/configerator" \
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
  * Initializing CfgrLiveCommitSyncConfig (glob)
  * Done initializing CfgrLiveCommitSyncConfig (glob)
  * Started importing git commits to Mononoke (glob)
  * Created ce435b03d4ef526648f8654c61e26ae5cc1069cc => ChangesetId(Blake2(f7cbf75d9c08ff96896ed2cebd0327aa514e58b1dd9901d50129b9e08f4aa062)) (glob)
  * Created 2c01e4a5658421e2bfcd08e31d9b69399319bcd3 => ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb)) (glob)
  * 2 bonsai changesets have been committed (glob)
  * Ref: Some("refs/heads/master"): Some(ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb))) (glob)
  * Added commits to Mononoke (glob)
  * Saving gitimported bonsai changesets (glob)
  * Saved gitimported bonsai changesets (glob)
  * Remapped ChangesetId(Blake2(f7cbf75d9c08ff96896ed2cebd0327aa514e58b1dd9901d50129b9e08f4aa062)) => ChangesetId(Blake2(a159bc614d2dbd07a5ecc6476156fa464b69e884d819bbc2e854ade3e4c353b9)) (glob)
  * Remapped ChangesetId(Blake2(f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb)) => ChangesetId(Blake2(a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5)) (glob)
  * Saving shifted bonsai changesets (glob)
  * Saved shifted bonsai changesets (glob)
  * Start deriving data types (glob)
  * Finished deriving data types (glob)
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
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists blame $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists deleted_manifest $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists fastlog $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists filenodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists fsnodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists hgchangesets $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5
  $ mononoke_admin derived-data exists unodes $BOOKMARK_NAME 2> /dev/null
  Derived: a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5

# Start Mononoke
  $ mononoke
  $ wait_for_mononoke

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
      "f7cbf75d9c08ff96896ed2cebd0327aa514e58b1dd9901d50129b9e08f4aa062",
      "f7708ed066b1c23591f862148e0386ec704a450e572154cc52f87ca0e394a0fb"
    ],
    "hg_sync_check_disabled": true,
    "import_stage": "PushCommit",
    "merged_cs_id": * (glob)
    "move_bookmark_commits_done": 1,
    "phab_check_disabled": true,
    "recovery_file_path": "$TESTTMP/repo-git/recovery_file.json",
    "shifted_bcs_ids": [
      "a159bc614d2dbd07a5ecc6476156fa464b69e884d819bbc2e854ade3e4c353b9",
      "a2e6329ed60e3dd304f53efd0f92c28b849404a47979fcf48bb43b6fe3a0cad5"
    ],
    "sleep_time": 5,
    "x_repo_check_disabled": false
  }

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo1 --noupdate -q
  $ cd repo1
  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files
  updating bookmark master_bookmark
  adding remote bookmark repo_import_new_repo
  $ hgmn up master_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (activating bookmark master_bookmark)

  $ log -r "all()"
  @    merging [draft;rev=5;*] (glob)
  ├─╮
  │ o  Add file3 [draft;rev=4;12e9a7555b29]
  │ │
  │ o  Add file1 and file2 [draft;rev=3;25f978935fdd]
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
