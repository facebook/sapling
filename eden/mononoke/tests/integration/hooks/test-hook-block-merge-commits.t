# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ hook_test_setup \
  > block_merge_commits <( \
  >   cat <<CONF
  > config_json='''{
  >  "disable_merge_bypass_on_bookmarks": [
  >     "bypass_disabled_on_this_bookmark"
  >  ],
  >  "commit_message_bypass_tag": "@merge-commit"
  > }'''
  > CONF
  > )




  $ hg up -q tip
  $ echo file1 > file1 && hg -q addremove && hg commit -m "file1"
  $ hg push -r . --to master_bookmark
  pushing rev 8e911067eac2 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
 

  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [26805a] C
  $ echo file2 > file2 && hg -q addremove && hg commit -m "file2"
  $ hg push -r . --to feature_bookmark --create
  pushing rev 0c7057776586 to destination mono:repo bookmark feature_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark feature_bookmark

  $ hg checkout master_bookmark
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge feature_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg commit -m "merge commit"
Should fail
  $ hg push -r . --to master_bookmark
  pushing rev abc80faa8edf to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for abc80faa8edf0e540ee85dafdf80bce11f68743f: You must not commit merge commits
  remote: 
  remote:   Root cause:
  remote:     hooks failed:
  remote:     block_merge_commits for abc80faa8edf0e540ee85dafdf80bce11f68743f: You must not commit merge commits
  remote: 
  remote:   Debug context:
  remote:     "hooks failed:\nblock_merge_commits for abc80faa8edf0e540ee85dafdf80bce11f68743f: You must not commit merge commits"
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg metaedit -m "commit message with bypass @merge-commit in message"
  $ hg push -r . --to master_bookmark
  pushing rev 5003ea84c31d to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
