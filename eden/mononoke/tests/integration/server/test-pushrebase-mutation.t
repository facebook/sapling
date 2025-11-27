# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ setconfig push.edenapi=true
  $ setup_common_config "blob_files"
  $ cd $TESTTMP

setup common configuration
  $ setconfig ui.ssh="\"$DUMMYSSH\"" mutation.date="0 0"
  $ enable amend

  $ testtool_drawdag -R repo << EOF
  > A
  > # bookmark: A master_bookmark
  > # modify: A base "base"
  > EOF
  A=46ac7d290bd2d2bcb9329136bb980807c3253fe7fd18ef8db501e4ea909e4f59

start mononoke
  $ start_and_wait_for_mononoke_server
clone the repo
  $ hg clone -q mono:repo client --noupdate
  $ cd client
  $ enable pushrebase

create a commit with mutation extras
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg commit -m 1
  $ echo 1a > 1 && hg amend -m 1a --config mutation.enabled=true --config mutation.record=true
  $ hg debugmutation
   *  700ced5d29aefe3930c87087f1b54ad4e1dc2e75 amend by test at 1970-01-01T00:00:00 from:
      a16777904428595a82e6b4cc8aeab9f03e39fe3b
  

pushrebase it directly onto master_bookmark - it will be rewritten without the mutation extras
  $ hg push -r . --to master_bookmark --config push.skip-cleanup-commits=true
  pushing rev 700ced5d29ae to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (f1b8d92077ea, 700ced5d29ae] (1 commit) to remote bookmark master_bookmark
  updated remote bookmark master_bookmark to a4b963b04373

  $ tglog
  @  700ced5d29ae '1a'
  │
  │ o  a4b963b04373 '1a'
  ├─╯
  o  f1b8d92077ea 'A'
  

  $ hg debugmutation -r master_bookmark
   *  a4b963b04373159e73986a6d89449e8f24576afc
  

create another commit on the base commit with mutation extras
  $ hg up 'min(all())'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 2 > 2 && hg add 2 && hg commit -m 2
  $ echo 2a > 2 && hg amend -m 2a --config mutation.enabled=true --config mutation.record=true
  $ hg debugmutation
   *  a0cd0d36df7f298998f3ab38e26d317283287587 amend by test at 1970-01-01T00:00:00 from:
      0f83d58b0e77f97d18ba8c8932f89fa50aa453f3
  

pushrebase it onto master_bookmark - it will be rebased and rewritten without the mutation extras
  $ hg push -r . --to master_bookmark --config push.skip-cleanup-commits=true
  pushing rev a0cd0d36df7f to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (f1b8d92077ea, a0cd0d36df7f] (1 commit) to remote bookmark master_bookmark
  updated remote bookmark master_bookmark to 8215b47f1213

  $ tglog
  @  a0cd0d36df7f '2a'
  │
  │ o  700ced5d29ae '1a'
  ├─╯
  │ o  8215b47f1213 '2a'
  │ │
  │ o  a4b963b04373 '1a'
  ├─╯
  o  f1b8d92077ea 'A'
  

  $ hg debugmutation -r master_bookmark
   *  8215b47f1213d1021665e85feaeeb4efc3d8fb0c
  
