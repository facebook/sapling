# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ . "${TEST_FIXTURES}/library-push-redirector.sh"

  $ setup_common_config
  $ GIT_REPO="${TESTTMP}/repo-git"
  $ GIT_SUBMODULE_REPO="${TESTTMP}/repo-submodule"
  $ HG_REPO="${TESTTMP}/repo"
  $ BLOB_TYPE="blob_files" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
  $ SKIP_CROSS_REPO_CONFIG=1 setup_configerator_configs
  $ enable_pushredirect 0
# Setup git repository to be used as submodule
  $ mkdir "$GIT_SUBMODULE_REPO"
  $ cd "$GIT_SUBMODULE_REPO"
  $ git init -q
  $ echo "foo" > foo
  $ git add foo
  $ git commit -am "Add foo"
  [master_bookmark (root-commit) 1c7ecd4] Add foo
   1 file changed, 1 insertion(+)
   create mode 100644 foo
  $ mkdir bar
  $ cd bar
  $ echo "qux" > qux
  $ cd ..
  $ git add bar/qux
  $ git commit -am "Add bar/qux"
  [master_bookmark 22b063b] Add bar/qux
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
  [master_bookmark (root-commit) ce435b0] Add file1 and file2
   2 files changed, 2 insertions(+)
   create mode 100644 file1
   create mode 100644 file2_repo/file2
  $ git -c protocol.file.allow=always submodule add ../repo-submodule
  Cloning into '$TESTTMP/repo-git/repo-submodule'...
  done.
  $ git add .
  $ git commit -am "Added git submodule"
  [master_bookmark 67328fd] Added git submodule
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




  $ GIT_MASTER_HASH=$(git log -n 1 --pretty=format:"%H" master_bookmark)


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

  $ enable_pushredirect 0 false false

  $ repo_import \
  > check-additional-setup-steps \
  > --disable-phabricator-check \
  > --bookmark-suffix "new_repo" \
  > --dest-bookmark master_bookmark
  * using repo "repo" repoid RepositoryId(0) (glob)
  * The importing bookmark name is: repo_import_new_repo. * (glob)
  * The destination bookmark name is: master_bookmark. * (glob)
  * There is no additional setup step needed! (glob)

# Import the repo
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
  GitRepo:$TESTTMP/repo-git commit 2 of 2 - Oid:67328fd4 => Bid:6f63fa96
  Added commits to Mononoke
  Commit 1/2: Remapped ChangesetId(Blake2(071d73e6b97823ffbde324c6147a785013f479157ade3f83c9b016c8f40c09de)) => ChangesetId(Blake2(4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a))
  Commit 2/2: Remapped ChangesetId(Blake2(6f63fa96348a0854cd77a4b70f5b9b776e963735f92e283b67123161f1bc7bcd)) => ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))
  Saving shifted bonsai changesets
  Saved shifted bonsai changesets
  Start deriving data types
  Finished deriving data types
  Start moving the bookmark
  Created bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } pointing to 4f830791a5ae7a2981d6c252d2be0bd7ebd3b1090080074b4b4bae6deb250b4a
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } to point to ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))
  Finished moving the bookmark
  Merging the imported commits into given bookmark, master_bookmark
  Done checking path conflicts
  Creating a merge bonsai changeset with parents: e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2, a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c
  Created merge bonsai: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd and changeset: BonsaiChangeset { inner: BonsaiChangesetMut { parents: [ChangesetId(Blake2(e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2)), ChangesetId(Blake2(a1740c3d4a0f8e012b12c0c93f5a69cc902fe7398d8f334ef202f33c32fc247c))], author: "user", author_date: DateTime(2005-04-02T21:37:00+01:00), committer: Some("user"), committer_date: Some(DateTime(2005-04-02T21:37:00+01:00)), message: "merging", hg_extra: {}, git_extra_headers: None, file_changes: {}, is_snapshot: false, git_tree_hash: None, git_annotated_tag: None }, id: ChangesetId(Blake2(97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd)) }
  Finished merging
  Running pushrebase
  Finished pushrebasing to 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  Set bookmark BookmarkKey { name: BookmarkName { bookmark: "repo_import_new_repo" }, category: Branch } to the merge commit: ChangesetId(Blake2(97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd))

# Check if we derived all the types for imported commits. Checking last one after bookmark move, before setting it to the merge commit.
  $ MERGE_PARENT_GIT="97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd"
  $ mononoke_admin derived-data -R repo exists -T changeset_info  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T blame  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T deleted_manifest  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T fastlog  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T filenodes  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T fsnodes  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T hgchangesets  -i $MERGE_PARENT_GIT
  Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd
  $ mononoke_admin derived-data -R repo exists -T unodes  -i $MERGE_PARENT_GIT
  Not Derived: 97a4e6df4c15db82ee1b428058a27b9fc274cb689f6eda481fdde33feff263bd

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

  $ hg clone -q mono:repo repo1 --noupdate
  $ cd repo1
  $ hg pull
  pulling from mono:repo
  $ hg up master_bookmark
  6 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg whereami
  db39bf064f102b6fdfa0f641cb08860a450f16af
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
  @    merging [public;rev=5;db39bf064f10] remote/master_bookmark
  ├─╮
  │ o  Added git submodule [public;rev=4;d5bd7c7af4df]
  │ │
  │ o  Add file1 and file2 [public;rev=3;4ad443ff73f0]
  │
  o  C [public;rev=2;d3b399ca8757]
  │
  o  B [public;rev=1;80521a640a0c]
  │
  o  A [public;rev=0;20ca2a4749a4]
  $

But using --stat crashes
  $ hg log -r "ancestors(master_bookmark)" --stat
  commit:      20ca2a4749a4
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  
   A |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      80521a640a0c
  user:        author
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
   B |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  commit:      d3b399ca8757
  user:        author
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
  
  commit:      db39bf064f10
  bookmark:    remote/master_bookmark
  hoistedname: master_bookmark
  user:        user
  date:        Sat Apr 02 21:37:00 2005 +0100
  summary:     merging
  
   new_dir/new_repo/.gitmodules      |  3 +++
   new_dir/new_repo/file1            |  1 +
   new_dir/new_repo/file2_repo/file2 |  1 +
   3 files changed, 5 insertions(+), 0 deletions(-)
  













  $ hg show 4ad443ff73f01bf1762918fa2be9c21cbdf038ea
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
