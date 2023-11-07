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
  $ echo n | repo_import \
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
  > --recovery-file-path "$GIT_REPO/recovery_file.json" &> /dev/null
  $ flush_mononoke_bookmarks

-- Checking imported files
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ ls imported
  file1

  $ quiet_grep "unsafe_sync_commit" -- backsync_large_to_small | sed 's/.*for \([0-9a-f]*\),.*/\1/' | uniq
  ee8403dabe5d6dd4bff1f7710bce9911e9652d401b28d93f46d17ca7b750a6ff
  cd3a7e1c74d34718522c7fd1218d5c25f252bf5e53418d5f31f2c75ffca6e008
  6f3d4345ba964c6b92f2379e989878b77f0215b89e58eaaa5e5949f9d1ffcabb
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
  

-- Import git repo again inside the small repo
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDLARGE"
  $ echo n | repo_import \
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
  > --recovery-file-path "$GIT_REPO/recovery_file.json" &> /dev/null
  $ flush_mononoke_bookmarks

-- Checking imported files
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull -q
  $ REPONAME=large-mon hgmn up -q master_bookmark

  $ ls smallrepofolder/imported2
  file1

  $ quiet_grep "unsafe_sync_commit" -- backsync_large_to_small | sed 's/.*for \([0-9a-f]*\),.*/\1/' | uniq
  598c0915137803605663980dad7367002e3f6f351aa85cd46e90b41f158454b3
  2b831e9e488065e1f05a4f1926bb74b1552dde0850ca4b75ee05d0d95e687b72
  9935491fa9d110909dc34622500a90a9efe687a0faea706923ef91e7e5c0954f
  23aba89fde8b4f197a84321478052a233219595afaa310e482dd9f45e0c0c3d2

  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ hg log -G -T '{desc}'
  @    merging2
  ├─╮
  │ o  Modify file1 B
  │ │
  │ o  Modify file1 A
  │ │
  │ o  Add file1
  │
  o  merging
  │
  o  first post-move commit
  │
  o  pre-move commit
  

  $ ls imported2
  file1
