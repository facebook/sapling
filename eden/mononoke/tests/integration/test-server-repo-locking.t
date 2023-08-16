# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ INFINITEPUSH_ALLOW_WRITES=true setup_common_config
  $ testtool_drawdag -R repo << EOF
  > A-B-C
  > # bookmark: C master
  > EOF
  A=aa53d24251ff3f54b1b2c29ae02826701b2abeb0079f1bb13b8434b54cd87675
  B=f8c75e41a0c4d29281df765f39de47bca1dcadfdc55ada4ccc2f6df567201658
  C=e32a1e342cdb1e38e88466b4c1a01ae9f410024017aa21dc0a1c5da6b3963bf2

  $ start_and_wait_for_mononoke_server

  $ mononoke_newadmin locking status
  repo                 Unlocked

Lock the repo
  $ mononoke_newadmin locking lock -R repo --reason "integration test"
  repo locked

Show it is locked
  $ mononoke_newadmin locking status
  repo                 Locked("integration test")

Can still clone the repo
  $ hgclone_treemanifest mononoke://$(mononoke_address)/repo repo-hg
  $ cd repo-hg
  $ enable infinitepush commitcloud pushrebase
  $ hg checkout -q '.^' 
  $ echo D > D
  $ hg commit -Aqm D

Can still push to commit cloud
  $ hgmn cloud backup
  backing up stack rooted at 9c00c53d25b3
  commitcloud: backed up 1 commit

Cannot push to the server
  $ hgmn push --to master
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     Repo is locked: integration test
  remote: 
  remote:   Root cause:
  remote:     Repo is locked: integration test
  remote: 
  remote:   Debug context:
  remote:     RepoLocked(
  remote:         "integration test",
  remote:     )
  abort: unexpected EOL, expected netstring digit
  [255]

Unlock the repo
  $ mononoke_newadmin locking unlock -R repo
  repo unlocked

Now we can push
  $ hgmn push --to master
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ hgmn pull -q
  $ tglogp
  o  1e21255e651f public 'D' master
  │
  │ @  9c00c53d25b3 draft 'D'
  │ │
  o │  d3b399ca8757 public 'C'
  ├─╯
  o  80521a640a0c public 'B'
  │
  o  20ca2a4749a4 public 'A'
  
