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
  $ quiet mononoke_modern_sync sync-once orig dest --start-id 0

  $ mononoke_admin mutable-counters --repo-name orig get modern_sync
  Some(2)
  $ cat  $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json 'Start sync process' .normal.log_tag .normal.repo .normal.run_id .int.start_id 2>&1 | grep -v 'null (null) cannot be matched'
  {
    "log_tag": "Start sync process",
    "repo": "orig",
    "run_id": *, (glob)
    "start_id": 0
  }
  $ cat  $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json '(Start|Done|Error) processing bookmark update entry' \
  > .normal.log_tag .normal.repo .normal.run_id \
  > .normal.bookmark_entry_bookmark_name .normal.bookmark_entry_from_changeset_id .normal.bookmark_entry_to_changeset_id .normal.bookmark_entry_reason \
  > .int.bookmark_entry_id .int.bookmark_entry_timestamp .int.bookmark_entry_commits_count .int.elapsed \
  > 2>&1 | grep -v 'null (null) cannot be matched'
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
    "bookmark_entry_commits_count": 1,
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
  $ cat  $TESTTMP/modern_sync_scuba_logs | summarize_scuba_json '(Start|Done|Error) processing changeset' \
  > .normal.log_tag .normal.repo \
  > .normal.bookmark_name .normal.changeset_id \
  > .int.elapsed \
  > 2>&1 | grep -v 'null (null) cannot be matched'
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "log_tag": "Start processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "53b034a90fe3002a707a7da9cdf6eac3dea460ad72f7c6969dfb88fd0e69f856",
    "elapsed": *, (glob)
    "log_tag": "Done processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304",
    "log_tag": "Start processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "8a9d572a899acdef764b88671c24b94a8b0780c1591a5a9bca97184c2ef0f304",
    "elapsed": *, (glob)
    "log_tag": "Done processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5",
    "log_tag": "Start processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "41deea4804cd27d1f4efbec135d839338804a5dfcaf364863bd0289067644db5",
    "elapsed": *, (glob)
    "log_tag": "Done processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3",
    "log_tag": "Start processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "ba1a2b3ca64cead35117cb2b707da1211cf43639ade917aee655f3875f4922c3",
    "elapsed": *, (glob)
    "log_tag": "Done processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8",
    "log_tag": "Start processing changeset",
    "repo": "orig"
  }
  {
    "bookmark_name": "master_bookmark",
    "changeset_id": "5b1c7130dde8e54b4285b9153d8e56d69fbf4ae685eaf9e9766cc409861995f8",
    "elapsed": *, (glob)
    "log_tag": "Done processing changeset",
    "repo": "orig"
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

// Try to re-sync and hit error cause bookmark can't be re-written
  $ with_stripped_logs mononoke_modern_sync sync-once orig dest --start-id 0
  Running sync-once loop
  Connecting to https://localhost:$LOCAL_PORT/edenapi/
  Established EdenAPI connection
  Initialized channels
  Calculating segments for entry 1
  Skipping 1 commits, starting sync of 0 commits 
  Moved bookmark with result SetBookmarkResponse { data: Ok(()) }
  Calculating segments for entry 2
  Skipping 4 commits, starting sync of 0 commits 
  Moved bookmark with result SetBookmarkResponse { data: Ok(()) }
