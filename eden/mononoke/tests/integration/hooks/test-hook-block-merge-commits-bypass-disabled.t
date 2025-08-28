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
  >  "disable_merge_bypass": true,
  >  "disable_merge_bypass_on_bookmarks": [
  >     "master_bookmark"
  >  ],
  >  "commit_message_bypass_tag": "@merge-commit"
  > }'''
  > CONF
  > )

  $ setconfig remotenames.selectivepulldefault=master_bookmark,feature_bookmark


  $ hg up -q tip
  $ echo file1 > file1 && hg -q addremove && hg commit -m "file1"
  $ hg push -r . --to master_bookmark
  pushing rev 3c7ceb974a6f to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark
 

  $ hg prev
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  [0a489c] c
  $ echo file2 > file2 && hg -q addremove && hg commit -m "file2"
  $ hg push -r . --to feature_bookmark --create
  pushing rev 71d10e5fbc60 to destination mono:repo bookmark feature_bookmark
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
  pushing rev 3b564bf6febc to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for 3b564bf6febcf080fa12039f93c70dfce5eaa796: You must not commit merge commits
  abort: unexpected EOL, expected netstring digit
  [255]

  $ hg metaedit -m "commit message with bypass @merge-commit in message"
  $ hg push -r . --to master_bookmark
  pushing rev 1275765c7903 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for 1275765c7903f2c109380a6d47bab8bebf29b7e9: This repository can't have merge commits
  abort: unexpected EOL, expected netstring digit
  [255]

Disable the repository level merge bypass disable to test bookmark level merge
bypass disable.
  $ hook_test_setup \
  > block_merge_commits <( \
  >   cat <<CONF
  > config_json='''{
  >  "disable_merge_bypass": false,
  >  "disable_merge_bypass_on_bookmarks": [
  >     "master_bookmark"
  >  ],
  >  "commit_message_bypass_tag": "@merge-commit"
  > }'''
  > CONF
  > )
  abort: destination 'repo2' is not empty
  $ force_update_configerator

  $ hg push -r . --to master_bookmark
  pushing rev 1275765c7903 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_merge_commits for 1275765c7903f2c109380a6d47bab8bebf29b7e9: This bookmark can't have merge commits
  abort: unexpected EOL, expected netstring digit
  [255]

