# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:modern_sync_bookmark_polling": true
  >   }
  > }
  > EOF

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
  >         "read": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "write": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"],
  >         "bypass_readonly": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   },
  >   "tiers": {
  >     "mirror_commit_upload": {
  >       "actions": {
  >         "mirror_upload": ["$CLIENT0_ID_TYPE:$CLIENT0_ID_DATA", "SERVICE_IDENTITY:server", "X509_SUBJECT_NAME:CN=localhost,O=Mononoke,C=US,ST=CA", "X509_SUBJECT_NAME:CN=client0,O=Mononoke,C=US,ST=CA"]
  >       }
  >     }
  >   }
  > }
  > ACLS

  $ REPOID=0 REPONAME=orig ACL_NAME=orig setup_common_config
  $ REPOID=1 REPONAME=dest ACL_NAME=dest setup_common_config
  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:orig orig
  $ cd orig
  $ drawdag <<EOF
  > B
  > |
  > A
  > EOF
  $ hg goto -q $A
  $ hg push -q -r . --to unrelated --create
  $ hg goto -q $B
  $ hg push -q -r . --to master_bookmark --create
  $ echo unrelated-only > unrelated-only
  $ hg add unrelated-only
  $ hg commit -qm "unrelated-only move"
  $ hg push -q -r . --to unrelated
  $ echo master-next > master-next
  $ hg add master-next
  $ hg commit -qm "next master move"
  $ hg push -q -r . --to master_bookmark
  $ quiet mononoke_admin mutable-counters -R orig set modern_sync 0

The bookmark mode reads only the two configured-bookmark rows, across the
intervening unrelated rows. It advances its own cursor while leaving the
rollback cursor frozen.

  $ quiet mononoke_modern_sync "--flatten-bul" sync-once orig dest
  $ mononoke_admin mutable-counters -R orig get modern_sync_by_bookmark_v1_branch_master_bookmark
  Some(4)
  $ mononoke_admin mutable-counters -R orig get modern_sync
  Some(0)
  $ mononoke_admin bookmarks -R dest list | cut -d " " -f2
  master_bookmark
  $ mononoke_admin bookmarks -R dest log master_bookmark -S hg | cut -d' ' -f1
  4
  2

Rollback must start after the bookmark-mode cursor, not the frozen legacy cursor.
Add one new master move so replaying from zero would mix already-applied moves with
a new suffix in one destination transaction.

  $ echo rollback-next > rollback-next
  $ hg add rollback-next
  $ hg commit -qm "move before rollback"
  $ hg push -q -r . --to master_bookmark
  $ merge_just_knobs <<EOF
  > {
  >   "bools": {
  >     "scm/mononoke:modern_sync_bookmark_polling": false
  >   }
  > }
  > EOF
  $ quiet mononoke_modern_sync "--flatten-bul" sync-once orig dest
  $ mononoke_admin mutable-counters -R orig get modern_sync
  Some(5)
  $ mononoke_admin mutable-counters -R orig get modern_sync_by_bookmark_v1_branch_master_bookmark
  Some(4)
  $ mononoke_admin bookmarks -R dest log master_bookmark -S hg | cut -d' ' -f1
  5
  4
  2
