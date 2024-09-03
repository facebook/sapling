# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
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
  $ hg push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147970
  $ hg bookmarks --remote
     default/master_bookmark   2fa5be0dd895

Push another commit, check that the globalrev is incrementing
  $ touch file2
  $ hg ci -Aqm commit2
  $ hg push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147971
  $ hg bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5


Check that we create a new bookmark that is a descendant of the globalrev bookmark
  $ hg push -q -r '.^' --to other_bookmark --create
  $ hg bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5
     default/other_bookmark    2fa5be0dd895

Check that we update bookmark to a descendant of the globalrev bookmark
  $ hg push -q -r . --to other_bookmark --force
  $ hg bookmarks --remote
     default/master_bookmark   7a3a1e2e51f5
     default/other_bookmark    7a3a1e2e51f5

Check that we cannot pushrebase on that bookmark
  $ touch file3
  $ hg ci -Aqm commit3
  $ hg push -r . --to other_bookmark
  pushing rev 9596b4eb01f6 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (7a3a1e2e51f5, 9596b4eb01f6] (1 commit) to remote bookmark other_bookmark
  abort: Server error: invalid request: This repository uses Globalrevs. Pushrebase is only allowed onto the bookmark 'master_bookmark', this push was for 'other_bookmark'
  [255]

Check that we cannot push to that bookmark if the commit is not a descendant
  $ touch file3
  $ hg ci -Aqm commit3
  [1]
  $ hg push -r . --to other_bookmark --force
  pushing rev 9596b4eb01f6 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other_bookmark
  moving remote bookmark other_bookmark from 7a3a1e2e51f5 to 9596b4eb01f6
  abort: server error: invalid request: Bookmark 'other_bookmark' can only be moved to ancestors of 'master_bookmark'
  [255]

Check that we cannot do a regular push to the globalrev bookmark either
  $ hg push -r . --to master_bookmark --force
  pushing rev 9596b4eb01f6 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from 7a3a1e2e51f5 to 9596b4eb01f6
  abort: server error: invalid request: Bookmark 'master_bookmark' can only be moved to ancestors of 'master_bookmark'
  [255]
