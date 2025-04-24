# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ hook_test_setup \
  > block_unclean_merge_commits <( \
  >   cat <<CONF
  > config_json='''{
  >  "only_check_branches_matching_regex": "master_bookmark"
  > }'''
  > CONF
  > )

  $ setconfig remotenames.selectivepulldefault=master_bookmark,feature_bookmark,new_bookmark_not_in_regex,feature_bookmark2

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
  $ echo file1differentcontent > file1 && hg -q addremove && hg commit -m "file1differentcontent"
  $ hg push -r . --to feature_bookmark --create
  pushing rev 63a68d44f9cf to destination mono:repo bookmark feature_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark feature_bookmark

  $ hg checkout master_bookmark
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge feature_bookmark
  merging file1
  warning: 1 conflicts while merging file1! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg resolve -t internal:local --all
  (no more unresolved files)
  $ hg commit -m "merge commit"
Should fail
  $ hg push -r . --to master_bookmark
  pushing rev 1688b90c1ac2 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_unclean_merge_commits for 1688b90c1ac2fa656a7c1def6f97126896c8dddd: The bookmark matching regex master_bookmark can't have merge commits with conflicts, even if they have been resolved
  abort: unexpected EOL, expected netstring digit
  [255]





  $ hg up -q tip
  $ echo file2 > file2 && hg -q addremove && hg commit -m "file2"
  $ hg push -r . --to new_bookmark_not_in_regex --create
  pushing rev 72703a82a860 to destination mono:repo bookmark new_bookmark_not_in_regex
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark new_bookmark_not_in_regex
 

  $ hg prev
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  [1688b9] merge commit
  $ echo file2differentcontent > file1 && hg -q addremove && hg commit -m "file2differentcontent"
  $ hg push -r . --to feature_bookmark2 --create
  pushing rev 5db8ccea674a to destination mono:repo bookmark feature_bookmark2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark feature_bookmark2

  $ hg checkout new_bookmark_not_in_regex
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge feature_bookmark2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg resolve -t internal:local --all
  (no more unresolved files)
  $ hg commit -m "merge commit"
Should succeed
  $ hg push -r . --to new_bookmark_not_in_regex
  pushing rev 82e904315b12 to destination mono:repo bookmark new_bookmark_not_in_regex
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark new_bookmark_not_in_regex
