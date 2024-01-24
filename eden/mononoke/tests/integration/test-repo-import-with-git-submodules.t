# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ GIT_SUBMODULE_REPO="${TESTTMP}/repo-submodule"
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
# Setup git repository to be used as submodule
  $ mkdir "$GIT_SUBMODULE_REPO"
  $ cd "$GIT_SUBMODULE_REPO"
  $ git init -q
  $ echo "foo" > foo
  $ git add foo
  $ git commit -am "Add foo"
  [master (root-commit) 1c7ecd4] Add foo
   1 file changed, 1 insertion(+)
   create mode 100644 foo
  $ mkdir bar
  $ cd bar
  $ echo "qux" > qux
  $ cd ..
  $ git add bar/qux
  $ git commit -am "Add bar/qux"
  [master 22b063b] Add bar/qux
   1 file changed, 1 insertion(+)
   create mode 100644 bar/qux
  $ git log
  commit 22b063b2f882d144e773c28f1d030715eccbe2c9
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Add bar/qux
  
  commit 1c7ecd42e00f23148eb3ec0488f5f093d4abedd6
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Add foo


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
  $ git -c protocol.file.allow=always submodule add ../repo-submodule
  Cloning into '$TESTTMP/repo-git/repo-submodule'...
  done.
  $ git add .
  $ git commit -am "Added git submodule" 
  [master 67328fd] Added git submodule
   2 files changed, 4 insertions(+)
   create mode 100644 .gitmodules
   create mode 160000 repo-submodule
  $ git log
  commit 67328fd43cc090474ba047aa5dccc86e2a08dff0
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Added git submodule
  
  commit ce435b03d4ef526648f8654c61e26ae5cc1069cc
  Author: mononoke <mononoke@mononoke>
  Date:   Sat Jan 1 00:00:00 2000 +0000
  
      Add file1 and file2
  $ GIT_MASTER_HASH=$(git log -n 1 --pretty=format:"%H" master)


# Run setup checker
  $ cd "$TESTTMP"
  $ repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
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

  $ repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
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
  $ with_stripped_logs repo_import \
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
  using repo "repo" repoid RepositoryId(0)
  Started importing git commits to Mononoke
  GitRepo:$TESTTMP/repo-git commit 1 of 2 - Oid:ce435b03 => Bid:071d73e6
  GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:67328fd4 => Bid:6f63fa96
  Added commits to Mononoke
  Commit 1/2: Remapped ChangesetId(Blake2(071d73e6b97823ffbde324c6147a785013f479157ade3f83c9b016c8f40c09de)) => ChangesetId(Blake2(4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a))
  Commit 2/2: Remapped ChangesetId(Blake2(6f63fa96348a0854cd77a4b70f5b9b776e963735f92e283b67123161f1bc7bcd)) => ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))
  Saving shifted bonsai changesets
  Saved shifted bonsai changesets
  Start deriving data types
  Finished deriving data types
  Start tailing segmented changelog
  Using the following segmented changelog heads: [Bookmark(BookmarkKey { name: BookmarkName { bookmark: "master_bookmark" }, category: Branch })]
  repo 0: SegmentedChangelogTailer initialized
  starting incremental update to segmented changelog
  iddag initialized, it covers 3 ids
  starting the actual update
  Adding hints for idmap_version 1
  idmap_version 1 has a full set of hints (4 unhinted IDs is less than chunk size of 5000)
  IdMap updated, IdDag updated
  segmented changelog version saved, idmap_version: 1, iddag_version: 858d30cfb08f0cc339b20663ddc4ed84ab67146515a386d735d5d903d2c67586
  successful incremental update to segmented changelog
  repo 0: SegmentedChangelogTailer is done
  Finished tailing segmented changelog
  Start moving the bookmark
  Created bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } pointing to 4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } to point to ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))
  Finished moving the bookmark
  Merging the imported commits into given bookmark, master_bookmark
  Done checking path conflicts
  Creating a merge bonsai changeset with parents: c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd, a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c
  Created merge bonsai: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2 and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))], author: "user", author_date: DateTime(2005-04-02T21:37:00+01:00), committer: Some("user"), committer_date: Some(DateTime(2005-04-02T21:37:00+01:00)), message: "merging", hg_extra: {}, git_extra_headers: None, file_changes: {}, is_snapshot: false, git_tree_hash: None, git_annotated_tag: None }, id: ChangesetId(Blake2(f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2)) }
  Finished merging
  Running pushrebase
  Finished pushrebasing to f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } to the merge commit: ChangesetId(Blake2(f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2))

# Check if we derived all the types for imported commits. Checking last one after bookmark move, before setting it to the merge commit.
  $ MERGE_PARENT_GIT="f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2"
  $ mononoke_newadmin derived-data -R repo exists -T changeset_info  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T blame  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T deleted_manifest  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T fastlog  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T filenodes  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T fsnodes  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T hgchangesets  -i $MERGE_PARENT_GIT
  Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2
  $ mononoke_newadmin derived-data -R repo exists -T unodes  -i $MERGE_PARENT_GIT
  Not Derived: f9b7b059f605feab3a96d8bfbcc2c9a43428496ed4ea1fb5625ea6cb41092dc2

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
    "git_merge_bcs_id": "6f63fa96348a0854cd77a4b70f5b9b776e963735f92e283b67123161f1bc7bcd",
    "git_merge_rev_id": "67328fd43cc090474ba047aa5dccc86e2a08dff0",
    "git_repo_path": "$TESTTMP/repo-git",
    "gitimport_bcs_ids": [
      "071d73e6b97823ffbde324c6147a785013f479157ade3f83c9b016c8f40c09de",
      "6f63fa96348a0854cd77a4b70f5b9b776e963735f92e283b67123161f1bc7bcd"
    ],
    "hg_sync_check_disabled": true,
    "import_stage": "PushCommit",
    "imported_cs_id": "a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c",
    "mark_not_synced_mapping": null,
    "merged_cs_id": * (glob)
    "move_bookmark_commits_done": 1,
    "phab_check_disabled": true,
    "recovery_file_path": "$TESTTMP/repo-git/recovery_file.json",
    "shifted_bcs_ids": [
      "4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a",
      "a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c"
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

  $ hg whereami
  abb3e8e8e71f1aa9ba229c72b7ee12a9825143e2
  $ tree
  .
  |-- A
  |-- B
  |-- C
  `-- new_dir
      `-- new_repo
          |-- file1
          `-- file2_repo
              `-- file2
  
  3 directories, 5 files


Normal log works
  $ log -r "ancestors(master_bookmark)"
  @    merging [draft;rev=5;abb3e8e8e71f]
  ├─╮
  │ o  Added git submodule [draft;rev=4;d5bd7c7af4df]
  │ │
  │ o  Add file1 and file2 [draft;rev=3;4ad443ff73f0]
  │
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $

But using --stat crashes
  $ hgedenapi log -r "ancestors(master_bookmark)" --stat
  commit:      426bada5c675
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
   A |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
   B |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      26805aba1e60
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     C
  
   C |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      4ad443ff73f0
  user:        mononoke <mononoke@mononoke>
  date:        Sat Jan 01 00:00:00 2000 +0000
  summary:     Add file1 and file2
  
   new_dir/new_repo/file1            |  1 +
   new_dir/new_repo/file2_repo/file2 |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
  
  commit:      d5bd7c7af4df
  user:        mononoke <mononoke@mononoke>
  date:        Sat Jan 01 00:00:00 2000 +0000
  summary:     Added git submodule
  
   new_dir/new_repo/.gitmodules |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)
  
  commit:      abb3e8e8e71f
  bookmark:    master_bookmark
  bookmark:    repo_import_new_repo
  user:        user
  date:        Sat Apr 02 21:37:00 2005 +0100
  summary:     merging
  
   new_dir/new_repo/.gitmodules      |  3 +++
   new_dir/new_repo/file1            |  1 +
   new_dir/new_repo/file2_repo/file2 |  1 +
   3 files changed, 5 insertions(+), 0 deletions(-)
  

  $ hgedenapi show 4ad443ff73f01bf1762918fa2be9c21cbdf038ea
  commit:      4ad443ff73f0
  user:        mononoke <mononoke@mononoke>
  date:        Sat Jan 01 00:00:00 2000 +0000
  files:       new_dir/new_repo/file1 new_dir/new_repo/file2_repo/file2
  description:
  Add file1 and file2
  
  
  diff -r 000000000000 -r 4ad443ff73f0 new_dir/new_repo/file1
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/new_dir/new_repo/file1	Sat Jan 01 00:00:00 2000 +0000
  @@ -0,0 +1,1 @@
  +this is file1
  diff -r 000000000000 -r 4ad443ff73f0 new_dir/new_repo/file2_repo/file2
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/new_dir/new_repo/file2_repo/file2	Sat Jan 01 00:00:00 2000 +0000
  @@ -0,0 +1,1 @@
  +this is file2
  
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
  cat: new_dir/new_repo/file3_repo/file3: No such file or directory
  [1]
