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
  $ quiet mononoke_modern_sync "" sync-once orig dest --start-id 0

  $ mononoke_admin mutable-counters --repo-name orig get modern_sync
  Some(2)
  $ cat  $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json 'Start sync process' .normal.log_tag .normal.repo .normal.run_id .int.start_id
  {
    "log_tag": "Start sync process",
    "repo": "orig",
    "run_id": *, (glob)
    "start_id": 0
  }
  $ cat  $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json '(Start|Done|Error) processing bookmark update entry' \
  > .normal.log_tag .normal.repo .normal.run_id \
  > .normal.bookmark_entry_bookmark_name .normal.bookmark_entry_from_changeset_id .normal.bookmark_entry_to_changeset_id .normal.bookmark_entry_reason \
  > .int.bookmark_entry_id .int.bookmark_entry_timestamp .int.bookmark_entry_commits_count .int.elapsed
  {
    "bookmark_entry_bookmark_name": "master_bookmark",
    "bookmark_entry_commits_count": 1,
    "bookmark_entry_id": 1,
    "bookmark_entry_reason": "push",
    "bookmark_entry_timestamp": *, (glob)
    "bookmark_entry_to_changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "log_tag": "Start processing bookmark update entry",
    "repo": "orig",
    "run_id": * (glob)
  }
  {
    "bookmark_entry_bookmark_name": "master_bookmark",
    "bookmark_entry_id": 1,
    "bookmark_entry_reason": "push",
    "bookmark_entry_timestamp": *, (glob)
    "bookmark_entry_to_changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "elapsed": *, (glob)
    "log_tag": "Done processing bookmark update entry",
    "repo": "orig",
    "run_id": * (glob)
  }
  {
    "bookmark_entry_bookmark_name": "master_bookmark",
    "bookmark_entry_commits_count": 4,
    "bookmark_entry_from_changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "bookmark_entry_id": 2,
    "bookmark_entry_reason": "push",
    "bookmark_entry_timestamp": *, (glob)
    "bookmark_entry_to_changeset_id": "5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8",
    "log_tag": "Start processing bookmark update entry",
    "repo": "orig",
    "run_id": * (glob)
  }
  {
    "bookmark_entry_bookmark_name": "master_bookmark",
    "bookmark_entry_from_changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "bookmark_entry_id": 2,
    "bookmark_entry_reason": "push",
    "bookmark_entry_timestamp": *, (glob)
    "bookmark_entry_to_changeset_id": "5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8",
    "elapsed": *, (glob)
    "log_tag": "Done processing bookmark update entry",
    "repo": "orig",
    "run_id": * (glob)
  }

# We can't make strict assertions because batching is timing-dependent. We can't at least check that we have at least one
# and that it has the fields we expect.
  $ cat $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json 'EdenAPI stats' \
  > .normal.log_tag .normal.repo \
  > .normal.endpoint \
  > .int.requests .int.downloaded_bytes .int.uploaded_bytes .int.elapsed .int.latency .int.download_speed .int.upload_speed \
  > | jq 'select(.endpoint == "upload/changesets/identical")' | head
  {
    "downloaded_bytes": \d+, (re)
    "elapsed": \d+, (re)
    "endpoint": "upload/changesets/identical",
    "latency": \d+, (re)
    "log_tag": "EdenAPI stats",
    "repo": "orig",
    "requests": 1,
    "uploaded_bytes": \d+ (re)
  }

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

// Try to re-sync. Notice how no missing commits are found, not only because of lookups but
// because, since we resume from latest bookmark, no commits are found given heads ends up being an ancestor of common.
// Since we force-set master in the first entry, second entry does indeed find commits but subsequently skips them due to lookups.
// Also there's only one bookmark moves instead of two due to the batching we use.
  $ mononoke_modern_sync "" sync-once orig dest --start-id 0
  [INFO] Running sync-once loop
  [INFO] [sync{repo=orig}] Opened SourceRepoArgs(Name("orig")) unredacted
  [INFO] [sync{repo=orig}] Starting sync from 0
  [INFO] [sync{repo=orig}] Connecting to https://localhost:$LOCAL_PORT/edenapi/, timeout 300s
  [INFO] [sync{repo=orig}] Established EdenAPI connection
  [INFO] [sync{repo=orig}] Initialized channels
  [INFO] [sync{repo=orig}] Read 2 entries
  [INFO] [sync{repo=orig}] 2 entries left after filtering
  [INFO] [sync{repo=orig}] mononoke_host="*" dogfooding=false (glob)
  [INFO] [sync{repo=orig}] Calculating segments for entry 1, from changeset Some(ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))) to changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), moved back by approx 4 commit(s)
  [INFO] [sync{repo=orig}] Done calculating segments for entry 1, from changeset Some(ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8))) to changeset ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856)), moved back by approx 4 commit(s) in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 1
  [INFO] [sync{repo=orig}] Calculating segments for entry 2, from changeset Some(ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))) to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), approx 4 commit(s)
  [INFO] [sync{repo=orig}] Done calculating segments for entry 2, from changeset Some(ChangesetId(Blake2(53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856))) to changeset ChangesetId(Blake2(5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8)), approx 4 commit(s) in *ms (glob)
  [INFO] [sync{repo=orig}] Resuming from latest entry checkpoint 0
  [INFO] [sync{repo=orig}] Skipping 0 batches from entry 2
  [INFO] [sync{repo=orig}] Starting sync of 0 missing commits, 4 were already synced
  [INFO] [sync{repo=orig}] Setting checkpoint from entry 2 to 0
  [INFO] [sync{repo=orig}] Setting bookmark master_bookmark from None to Some(HgChangesetId(HgNodeHash(Sha1(8c3947e5d8bd4fe70259eca001b8885651c75850))))
  [INFO] [sync{repo=orig}] Moved bookmark with result SetBookmarkResponse { data: Ok(()) }
  [INFO] [sync{repo=orig}] Marking entry 2 as done
