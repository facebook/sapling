# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ DISALLOW_NON_PUSHREBASE=1 GLOBALREVS_PUBLISHING_BOOKMARK=master_bookmark EMIT_OBSMARKERS=1 BLOB_TYPE="blob_files" default_setup
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2
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
     remote/master_bookmark           98a2c4b193c12e2d9a6ed86b03db2e90f0df0622

Push another commit, check that the globalrev is incrementing
  $ touch file2
  $ hg ci -Aqm commit2
  $ hg push -q -r . --to master_bookmark
  $ hg log -r . -T '{extras % "{extra}\n"}'
  branch=default
  global_rev=1000147971
  $ hg bookmarks --remote
     remote/master_bookmark           8ec880e752653069526770b8b9043725000cdc26


Check that we create a new bookmark that is a descendant of the globalrev bookmark
  $ hg push -q -r '.^' --to other_bookmark --create
  $ hg bookmarks --remote
     remote/master_bookmark           8ec880e752653069526770b8b9043725000cdc26
     remote/other_bookmark            98a2c4b193c12e2d9a6ed86b03db2e90f0df0622

Check that we update bookmark to a descendant of the globalrev bookmark
  $ hg push -q -r . --to other_bookmark --force
  $ hg bookmarks --remote
     remote/master_bookmark           8ec880e752653069526770b8b9043725000cdc26
     remote/other_bookmark            8ec880e752653069526770b8b9043725000cdc26

Check that we cannot pushrebase on that bookmark
  $ touch file3
  $ hg ci -Aqm commit3
  $ hg push -r . --to other_bookmark
  pushing rev e20c031363a7 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (8ec880e75265, e20c031363a7] (1 commit) to remote bookmark other_bookmark
  abort: Server error: invalid request: This repository uses Globalrevs. Pushrebase is only allowed onto the bookmark 'master_bookmark', this push was for 'other_bookmark'
  [255]

Check that we cannot push to that bookmark if the commit is not a descendant
  $ touch file3
  $ hg ci -Aqm commit3
  [1]
  $ hg push -r . --to other_bookmark --force
  pushing rev e20c031363a7 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark other_bookmark
  moving remote bookmark other_bookmark from 8ec880e75265 to e20c031363a7
  abort: server error: invalid request: Bookmark 'other_bookmark' can only be moved to ancestors of 'master_bookmark'
  [255]

Check that we cannot do a regular push to the globalrev bookmark either
  $ hg push -r . --to master_bookmark --force
  pushing rev e20c031363a7 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from 8ec880e75265 to e20c031363a7
  abort: server error: invalid request: Bookmark 'master_bookmark' can only be moved to ancestors of 'master_bookmark'
  [255]
