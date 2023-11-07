# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

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

-- Import git repo
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDLARGE"
  $ repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "imported" \
  > --batch-size 3 \
  > --bookmark-suffix "imported" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master \
  > --commit-author user \
  > --commit-message "merging" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "large_only" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json" &> /dev/null
  $ flush_mononoke_bookmarks

-- Checking imported files
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ ls imported
  file1

  $ quiet_grep "unsafe_sync_commit" -- backsync_large_to_small | sed 's/.*for \([0-9a-f]*\),.*/\1/' | uniq
  e7822e32dbc25ae9781e0077bc195338547e84dc719402327d076fc90aaeb2d8

  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg log -G -T '{desc}'
  @  merging
  │
  o  first post-move commit
  │
  o  pre-move commit
  

-- Try to import git repo again inside the small repo's directory (it should fail).
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDLARGE"
  $ repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "smallrepofolder/imported2" \
  > --batch-size 3 \
  > --bookmark-suffix "imported2" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark master_bookmark \
  > --git-merge-rev-id master \
  > --commit-author user \
  > --commit-message "merging2" \
  > --commit-date-rfc3339 "1970-01-01T00:00:00Z" \
  > --mark-not-synced-mapping "large_only" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json"
  *] using repo "large-mon" repoid RepositoryId(0) (glob)
  *] Execution error: Small repo 1 default prefix smallrepofolder overlaps with import destination smallrepofolder/imported2 (glob)
  Error: Execution failed
  [1]

-- Now land some commits into the large repo
  $ cd "$TESTTMP/large-hg-client"
  $ echo data > smallrepofolder/newfile
  $ hg add smallrepofolder/newfile
  $ hg commit -m "Add new file in small repo"
  $ echo file1c > imported/file1
  $ hg commit -m "Modify file1 C"
  $ REPONAME=large-mon hgmn push -q --to master_bookmark

  $ quiet_grep "unsafe_sync_commit" -- backsync_large_to_small | sed 's/.*for \([0-9a-f]*\),.*/\1/' | uniq
  7e8fc15d33370bd32761d6f0fc383418ec58406d5a23b413ee0c3369c6f72d8e
  20ea025c3dfee5ae8b90d7d953a9a522d36eadc8d74d8ed9b2afa62a4d41314b

-- Check that the first commit was backsynced correctly.  The second should not be backsynced.
  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg log -G -T '{desc}'
  @  Add new file in small repo
  │
  o  merging
  │
  o  first post-move commit
  │
  o  pre-move commit
  
