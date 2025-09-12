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

  $ start_and_wait_for_mononoke_server

  $ hg clone -q mono:orig orig
  $ cd orig
  $ drawdag << EOS
  > E # E/dir1/dir2/fifth = abcdefg\n
  > |
  > D # D/dir1/dir2/forth = abcdef\n
  > |
  > C # C/dir1/dir2/third = abcde\n (copied from dir1/dir2/first)
  > |
  > B # B/dir1/dir2/second = abcd\n
  > |
  > A # A/dir1/dir2/first = abc\n
  > EOS


  $ hg goto A -q
  $ hg push -r . --to master_bookmark -q --create

  $ hg goto E -q
  $ hg push -r . --to master_bookmark -q

  $ hg log > $TESTTMP/hglog.out

Sync all bookmarks moves
  $ mononoke_modern_sync --flatten-bul sync-once orig dest  --start-id 0 2>&1 | grep -v "Upload" | grep -v "checkpoint"
  [INFO] Running sync-once loop
  [INFO] [sync{repo=orig}] Opened SourceRepoArgs(Name("orig")) unredacted
  [INFO] [sync{repo=orig}] Starting sync from 0
  [INFO] [sync{repo=orig}] Connecting to https://localhost:$LOCAL_PORT/edenapi/, timeout 300s
  [INFO] [sync{repo=orig}] Established EdenAPI connection
  [INFO] [sync{repo=orig}] Initialized channels
  [INFO] [sync{repo=orig}] Read 2 entries
  [INFO] [sync{repo=orig}] 2 entries left after filtering
  [INFO] [sync{repo=orig}] Grouped 2 entries into 1 macro-entries
  [INFO] [sync{repo=orig}] mononoke_host="*" dogfooding=false (glob)
  [INFO] [sync{repo=orig}] Calculating segments for entry 2, from changeset None to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), to generation 5
  [INFO] [sync{repo=orig}] Done calculating segments for entry 2, from changeset None to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), to generation 5 in *ms (glob)
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 2
  [INFO] [sync{repo=orig}] Starting sync of 5 missing commits, 0 were already synced
  [INFO] [sync{repo=orig}] Setting bookmark master_bookmark from None to Some(HgChangesetId(HgNodeHash(Sha1(8c3947e5d8bd4fe70259eca001b8885651c75850))))
  [INFO] [sync{repo=orig}] Moved bookmark with result SetBookmarkResponse { data: Ok(()) }
  [INFO] [sync{repo=orig}] Marking entry 2 as done


  $ mononoke_admin mutable-counters --repo-name orig get modern_sync
  Some(2)

  $ cd ..

  $ hg clone -q mono:dest dest --noupdate
  $ cd dest
  $ hg pull
  pulling from mono:dest

  $ hg log > $TESTTMP/hglog2.out
  $ hg up master_bookmark
  10 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls dir1/dir2
  fifth
  first
  forth
  second
  third

  $ diff  $TESTTMP/hglog.out  $TESTTMP/hglog2.out

  $ mononoke_admin repo-info  --repo-name dest --show-commit-count
  Repo: dest
  Repo-Id: 1
  Main-Bookmark: master (not set)
  Commits: 5 (Public: 0, Draft: 5)
