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
  $ log
  o  file1differentcontent [public;rev=281474976710656;63a68d44f9cf] remote/feature_bookmark
  │
  │ @  file1 [public;rev=3;3c7ceb974a6f] remote/master_bookmark
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $
  $ hg merge feature_bookmark
  merging file1
  warning: 1 conflicts while merging file1! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
# Let us resolve the conflict with completely new data
  $ echo "file 1 unique content" > file 1
  $ hg resolve --mark --all
  (no more unresolved files)
  $ hg commit -m "merge commit"
  $ log
  @    merge commit [draft;rev=281474976710657;9810592ad7f8]
  ├─╮
  │ o  file1differentcontent [public;rev=281474976710656;63a68d44f9cf] remote/feature_bookmark
  │ │
  o │  file1 [public;rev=3;3c7ceb974a6f] remote/master_bookmark
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $

# The push sould fail as the merge commit introduced completely new data.
  $ hg push -r . --to master_bookmark
  pushing rev 9810592ad7f8 to destination mono:repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     hooks failed:
  remote:     block_unclean_merge_commits for 9810592ad7f82df90bcf309012667074750d9204: The bookmark matching regex master_bookmark can't have merge commits with conflicts, even if they have been resolved
  abort: unexpected EOL, expected netstring digit
  [255]

# Let us resolve the conflict by taking all the changes from one of the parents. The push will succeed.
  $ echo file1 > file1
  $ hg amend
  $ log
  @    merge commit [draft;rev=281474976710658;c642db156193]
  ├─╮
  │ o  file1differentcontent [public;rev=281474976710656;63a68d44f9cf] remote/feature_bookmark
  │ │
  o │  file1 [public;rev=3;3c7ceb974a6f] remote/master_bookmark
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $
  $ hg push -r . --to master_bookmark
  pushing rev c642db156193 to destination mono:repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master_bookmark

# Let us set up  new case on bookmarks that are not covered by the regex from the hook config
  $ hg up -q tip
  $ echo file2 > file2 && hg -q addremove && hg commit -m "file2"
  $ log
  @  file2 [draft;rev=281474976710657;ccc9b0a84d87]
  │
  o    merge commit [public;rev=5;c642db156193] remote/master_bookmark
  ├─╮
  │ o  file1differentcontent [public;rev=4;63a68d44f9cf] remote/feature_bookmark
  │ │
  o │  file1 [public;rev=3;3c7ceb974a6f]
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $
  $ hg push -r . --to new_bookmark_not_in_regex --create
  pushing rev ccc9b0a84d87 to destination mono:repo bookmark new_bookmark_not_in_regex
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark new_bookmark_not_in_regex


  $ hg prev
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  [c642db] merge commit
  $ echo file2differentcontent > file1 && hg -q addremove && hg commit -m "file2differentcontent"
  $ log
  @  file2differentcontent [draft;rev=281474976710658;669e5b2aeafd]
  │
  │ o  file2 [public;rev=281474976710657;ccc9b0a84d87] remote/new_bookmark_not_in_regex
  ├─╯
  o    merge commit [public;rev=5;c642db156193] remote/master_bookmark
  ├─╮
  │ o  file1differentcontent [public;rev=4;63a68d44f9cf] remote/feature_bookmark
  │ │
  o │  file1 [public;rev=3;3c7ceb974a6f]
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $
  $ hg push -r . --to feature_bookmark2 --create
  pushing rev 669e5b2aeafd to destination mono:repo bookmark feature_bookmark2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  exporting bookmark feature_bookmark2

  $ hg checkout new_bookmark_not_in_regex
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge feature_bookmark2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg resolve -t internal:local --all
  (no more unresolved files)
  $ hg commit -m "merge commit2"
  $ log
  @    merge commit2 [draft;rev=281474976710659;30e8593290e2]
  ├─╮
  │ o  file2differentcontent [public;rev=281474976710658;669e5b2aeafd] remote/feature_bookmark2
  │ │
  o │  file2 [public;rev=281474976710657;ccc9b0a84d87] remote/new_bookmark_not_in_regex
  ├─╯
  o    merge commit [public;rev=5;c642db156193] remote/master_bookmark
  ├─╮
  │ o  file1differentcontent [public;rev=4;63a68d44f9cf] remote/feature_bookmark
  │ │
  o │  file1 [public;rev=3;3c7ceb974a6f]
  ├─╯
  o  c [public;rev=2;0a489c6e2d2c]
  │
  o  b [public;rev=1;fd8f618199ae]
  │
  o  a [public;rev=0;623cdcdd7586]
  $
# The push should succeed as the bookmark is not in the regex
  $ hg push -r . --to new_bookmark_not_in_regex
  pushing rev 30e8593290e2 to destination mono:repo bookmark new_bookmark_not_in_regex
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark new_bookmark_not_in_regex
