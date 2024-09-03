# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config "blob_files"
  $ cd $TESTTMP

setup common configuration
  $ setconfig ui.ssh="\"$DUMMYSSH\"" mutation.date="0 0"
  $ enable amend

  $ hginit_treemanifest repo
  $ cd repo
  $ echo base > base
  $ hg commit -Aqm base
  $ hg bookmark master -r tip

blobimport
  $ cd $TESTTMP
  $ blobimport repo/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
clone the repo
  $ hg clone -q mono:repo client --noupdate
  $ cd client
  $ enable pushrebase remotenames

create a commit with mutation extras
  $ hg up -q "min(all())"
  $ echo 1 > 1 && hg add 1 && hg commit -m 1
  $ echo 1a > 1 && hg amend -m 1a --config mutation.enabled=true --config mutation.record=true
  $ hg debugmutation
   *  6ad95cdc8ab9aab92b341e8a7b90296d04885b30 amend by test at 1970-01-01T00:00:00 from:
      f0161ad23099c690115006c21e96f780f5d740b6
  
pushrebase it directly onto master - it will be rewritten without the mutation extras
  $ hg push -r . --to master --config push.skip-cleanup-commits=true
  pushing rev 6ad95cdc8ab9 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (d20a80d4def3, 6ad95cdc8ab9] (1 commit) to remote bookmark master
  updated remote bookmark master to a05b3505b7d1

  $ tglog
  o  a05b3505b7d1 '1a'
  │
  │ @  6ad95cdc8ab9 '1a'
  ├─╯
  o  d20a80d4def3 'base'
  
  $ hg debugmutation -r master
   *  a05b3505b7d1aac5fd90b09a5f014822647ec205
  
create another commit on the base commit with mutation extras
  $ hg up 'min(all())'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo 2 > 2 && hg add 2 && hg commit -m 2
  $ echo 2a > 2 && hg amend -m 2a --config mutation.enabled=true --config mutation.record=true
  $ hg debugmutation
   *  fd935a5d42c4be474397d87ab7810b0b006722af amend by test at 1970-01-01T00:00:00 from:
      1b9fe529321657f93e84f23afaf9c855b9af34ff
  
pushrebase it onto master - it will be rebased and rewritten without the mutation extras
  $ hg push -r . --to master --config push.skip-cleanup-commits=true
  pushing rev fd935a5d42c4 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (d20a80d4def3, fd935a5d42c4] (1 commit) to remote bookmark master
  updated remote bookmark master to 7042a534cddc

  $ tglog
  o  7042a534cddc '2a'
  │
  │ @  fd935a5d42c4 '2a'
  │ │
  o │  a05b3505b7d1 '1a'
  ├─╯
  │ o  6ad95cdc8ab9 '1a'
  ├─╯
  o  d20a80d4def3 'base'
  
  $ hg debugmutation -r master
   *  7042a534cddcd761aeea38446ce39590634568e8
  
