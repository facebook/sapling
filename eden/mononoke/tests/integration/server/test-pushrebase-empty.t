# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ setconfig push.edenapi=true
  $ BLOB_TYPE="blob_sqlite" default_setup_drawdag
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

Push single empty commit
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ hg revert -r .^ 1
  $ hg commit --amend
  $ hg show
  commit:      6be0d6e308ce
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  description:
  1
  
  
  



  $ hg push -r . --to master_bookmark
  pushing rev 6be0d6e308ce to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 1 changeset
  pushrebasing stack (20ca2a4749a4, 6be0d6e308ce] (1 commit) to remote bookmark master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 8523ebae9c7b

Push empty and non-empty commit in a stack
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg ci -m 1
  $ echo 2 > 2 && hg add 2 && hg ci -m 2
  $ hg revert -r .^ 2
  $ hg commit --amend
  $ hg push -r . --to master_bookmark
  pushing rev e7ecf1de70fd to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 2 changesets
  pushrebasing stack (20ca2a4749a4, e7ecf1de70fd] (2 commits) to remote bookmark master_bookmark
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to 2f4070d136e3

Push stack of empty commits
  $ hg up -q tip
  $ echo 1 > 11 && hg add 11 && hg ci -m emptystack1
  $ hg revert -r .^ 11
  $ hg commit --amend
  $ echo 1 > 111 && hg add 111 && hg ci -m emptystack2
  $ hg revert -r .^ 111
  $ hg commit --amend
  $ hg push -r . --to master_bookmark
  pushing rev bec6b3705d41 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 2 commits for upload
  edenapi: queue 0 files for upload
  edenapi: queue 0 trees for upload
  edenapi: uploaded 2 changesets
  pushrebasing stack (2f4070d136e3, bec6b3705d41] (2 commits) to remote bookmark master_bookmark
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark master_bookmark to bec6b3705d41
