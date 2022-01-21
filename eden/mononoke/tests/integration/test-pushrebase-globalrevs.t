# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
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
  $ hg up -q master_bookmark

Push commit, check a globalrev was assigned
  $ touch file1
  $ hg ci -Aqm commit1
  $ hgmn push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147970
  $ hgmn bookmarks --remote
     default/master_bookmark   2fa5be0dd895

Push another commit, check that the globalrev is incrementing
  $ touch file2
  $ hg ci -Aqm commit2
  $ hgmn push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147971
  $ hgmn bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5


Check that we create a new bookmark that is a descendant of the globalrev bookmark
  $ hgmn push -q -r '.^' --to other_bookmark --create
  $ hgmn bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5
     default/other_bookmark    2fa5be0dd895

Check that we update bookmark to a descendant of the globalrev bookmark
  $ hgmn push -q -r . --to other_bookmark --force
  $ hgmn bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5
     default/other_bookmark    7a3a1e2e51f5

Check that we cannot pushrebase on that bookmark
  $ touch file3
  $ hg ci -Aqm commit3
  $ hgmn push -r . --to other_bookmark
  pushing rev 9596b4eb01f6 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     This repository uses Globalrevs. Pushrebase is only allowed onto the bookmark 'master_bookmark', this push was for 'other_bookmark'
  remote: 
  remote:   Root cause:
  remote:     This repository uses Globalrevs. Pushrebase is only allowed onto the bookmark 'master_bookmark', this push was for 'other_bookmark'
  remote: 
  remote:   Debug context:
  remote:     PushrebaseInvalidGlobalrevsBookmark {
  remote:         bookmark: BookmarkName {
  remote:             bookmark: "other_bookmark",
  remote:         },
  remote:         globalrevs_publishing_bookmark: BookmarkName {
  remote:             bookmark: "master_bookmark",
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Check that we cannot push to that bookmark if the commit is not a descendant
  $ touch file3
  $ hg ci -Aqm commit3
  [1]
  $ hgmn push -r . --to other_bookmark --force
  pushing rev 9596b4eb01f6 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark other_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a force pushrebase
  remote: 
  remote:   Root cause:
  remote:     Bookmark 'other_bookmark' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Bookmark 'other_bookmark' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a force pushrebase",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: RequiresAncestorOf {
  remote:                 bookmark: BookmarkName {
  remote:                     bookmark: "other_bookmark",
  remote:                 },
  remote:                 descendant_bookmark: BookmarkName {
  remote:                     bookmark: "master_bookmark",
  remote:                 },
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]

Check that we cannot do a regular push to the globalrev bookmark either
  $ hgmn push -r . --to master_bookmark --force
  pushing rev 9596b4eb01f6 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark master_bookmark
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a force pushrebase
  remote: 
  remote:   Root cause:
  remote:     Bookmark 'master_bookmark' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Caused by:
  remote:     Failed to move bookmark
  remote:   Caused by:
  remote:     Bookmark 'master_bookmark' can only be moved to ancestors of 'master_bookmark'
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a force pushrebase",
  remote:         source: Error {
  remote:             context: "Failed to move bookmark",
  remote:             source: RequiresAncestorOf {
  remote:                 bookmark: BookmarkName {
  remote:                     bookmark: "master_bookmark",
  remote:                 },
  remote:                 descendant_bookmark: BookmarkName {
  remote:                     bookmark: "master_bookmark",
  remote:                 },
  remote:             },
  remote:         },
  remote:     }
  abort: unexpected EOL, expected netstring digit
  [255]
