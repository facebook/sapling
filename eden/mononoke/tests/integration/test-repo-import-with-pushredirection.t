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
  $ XREPOSYNC=1 init_large_small_repo
  Adding synced mapping entry
  Starting Mononoke server

Before the change
-- push to a small repo
  $ quiet mononoke_admin_source_target $REPOIDLARGE $REPOIDSMALL crossrepo \
  > pushredirection prepare-rollout

  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF

  $ force_update_configerator

  $ cd "$TESTTMP/small-hg-client"
  $ REPONAME=small-mon hgmn pull -q
  $ REPONAME=small-mon hgmn up -q master_bookmark
  $ mkdir -p non_path_shifting
  $ echo a > foo
  $ echo b > non_path_shifting/bar
  $ hg ci -Aqm "before config change"
  $ REPONAME=small-mon hgmn push -r . --to new_bookmark --create
  pushing rev bc6a206054d0 to destination mononoke://$LOCALIP:$LOCAL_PORT/small-mon bookmark new_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark new_bookmark

  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": false
  >    }
  >   }
  > }
  > EOF

-- Make a version change
  $ update_commit_sync_map_first_option
  $ mononoke_admin_source_target $REPOIDLARGE $REPOIDSMALL crossrepo pushredirection change-mapping-version \
  > --author author \
  > --large-repo-bookmark master_bookmark \
  > --version-name new_version &> /dev/null
  $ mononoke_admin_source_target $REPOIDLARGE $REPOIDSMALL crossrepo \
  > pushredirection prepare-rollout &> /dev/null

  $ cat > "$PUSHREDIRECT_CONF/enable" <<EOF
  > {
  > "per_repo": {
  >   "1": {
  >      "draft_push": false,
  >      "public_push": true
  >    }
  >   }
  > }
  > EOF
  $ force_update_configerator

-- Setup git repository
  $ cd "$TESTTMP"
  $ export GIT_REPO=git_repo
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

-- Import git repo
  $ cd "$TESTTMP"
  $ REPOID="$REPOIDSMALL"
  $ echo "$REPOID"
  1
  $ echo n | repo_import \
  > import \
  > "$GIT_REPO" \
  > --dest-path "new_dir/new_repo" \
  > --batch-size 3 \
  > --bookmark-suffix "new_repo" \
  > --disable-phabricator-check \
  > --disable-hg-sync-check \
  > --dest-bookmark new_bookmark \
  > --git-merge-rev-id master \
  > --commit-author user \
  > --commit-message "merging" \
  > --recovery-file-path "$GIT_REPO/recovery_file.json" &> /dev/null
  $ flush_mononoke_bookmarks

-- Checking imported files
  $ cd "$TESTTMP/large-hg-client"
  $ REPONAME=large-mon hgmn pull
  pulling from mononoke://$LOCALIP:$LOCAL_PORT/large-mon
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ REPONAME=large-mon hgmn up bookprefix/new_bookmark
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ ls smallrepofolder
  file.txt
  filetoremove
  foo
  new_dir
