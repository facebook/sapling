# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ cat >> "$ACL_FILE" << ACLS
  > {
  >   "repos": {
  >     "orig": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     },
  >     "dest": {
  >       "actions": {
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >          "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   },
  >   "tiers": {
  >     "mirror_commit_upload": {
  >       "actions": {
  >         "mirror_upload": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA","SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ REPOID=0 REPONAME=orig ACL_NAME=orig setup_common_config
  $ REPOID=1 REPONAME=dest ACL_NAME=dest setup_common_config

  $ testtool_drawdag -R orig --derive-all --print-hg-hashes <<EOF
  > A-B-C
  > # bookmark: C master_bookmark
  > EOF
  A=20ca2a4749a439b459125ef0f6a4f26e88ee7538
  B=80521a640a0c8f51dcc128c2658b224d595840ac
  C=d3b399ca8757acdb81c3681b052eb978db6768d8

  $ start_and_wait_for_mononoke_server
  $ hg clone -q mono:orig orig
  $ cd orig
  $ hg up -q $C

Create a merge commit
  $ echo d > d
  $ hg commit -Aqm 'add d'
  $ D=$(hg log -r . -T '{node}')
  $ hg co -q $C
  $ echo e > e
  $ hg commit -Aqm 'add e'
  $ E=$(hg log -r . -T '{node}')
  $ hg merge -q $D
  $ hg commit -m "merge commit!!!"
  $ MERGE=$(hg log -r . -T '{node}')
  $ echo f > f
  $ hg commit -Aqm 'add f'
  $ F=$(hg log -r . -T '{node}')
  $ hg bookmark master_bookmark
  $ hg push -r . --to master_bookmark -q
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  @  f66acfddbf0932115801e7918d2579d732835a30 add f
  │
  o    2de0897e2852e9d1083f67dd35e4a8953f6cd674 merge commit!!!
  ├─╮
  │ o  eadd9514d3871766f4d3bbc0388f20ba2d559f28 add e
  │ │
  o │  a3308cc063dc2fdc0915f5f09a2d784915c305c5 add d
  ├─╯
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
Sync all bookmarks moves
  $ quiet mononoke_modern_sync "" sync-once orig dest --start-id 0

Clone and verify the destination repo
  $ cd ..
  $ hg clone -q mono:dest dest
  $ cd dest
  $ hg log -G -T '{node} {desc}\n' -r "all()"
  @  f66acfddbf0932115801e7918d2579d732835a30 add f
  │
  o    2de0897e2852e9d1083f67dd35e4a8953f6cd674 merge commit!!!
  ├─╮
  │ o  a3308cc063dc2fdc0915f5f09a2d784915c305c5 add d
  │ │
  o │  eadd9514d3871766f4d3bbc0388f20ba2d559f28 add e
  ├─╯
  o  d3b399ca8757acdb81c3681b052eb978db6768d8 C
  │
  o  80521a640a0c8f51dcc128c2658b224d595840ac B
  │
  o  20ca2a4749a439b459125ef0f6a4f26e88ee7538 A
  
