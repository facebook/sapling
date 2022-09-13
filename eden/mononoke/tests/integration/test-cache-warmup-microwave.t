# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

Setup repository

  $ export CACHE_WARMUP_BOOKMARK="master_bookmark"
  $ export CACHE_WARMUP_MICROWAVE=1
  $ BLOB_TYPE="blob_files" quiet default_setup

Check that Mononoke booted despite the lack of microwave snapshot

  $ wait_for_mononoke_cache_warmup
  $ grep microwave "$TESTTMP/mononoke.out"
  * microwave: cache warmup failed: "Snapshot is missing", repo: repo (glob)

Kill Mononoke

  $ killandwait "$MONONOKE_PID"
  $ truncate -s 0 "$TESTTMP/mononoke.out"

Delete filenodes

  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM filenodes;";

Regenerate microwave snapshot. This will fail because we have no derived data:

  $ microwave_builder --log-level ERROR blobstore
  * Execution error: Bookmark master_bookmark has no derived data (glob)
  Error: Execution failed
  [1]

Derive data, then regenerate microwave snapshot:

  $ quiet mononoke_newadmin dump-changesets -R repo --out-filename "$TESTTMP/prefetched_commits" fetch-public
  $ quiet backfill_derived_data backfill --prefetched-commits-path "$TESTTMP/prefetched_commits" filenodes
  $ quiet microwave_builder --debug blobstore

Start Mononoke again, check that the microwave snapshot was used

  $ start_and_wait_for_mononoke_server
  $ wait_for_mononoke_cache_warmup
  $ grep primed "$TESTTMP/mononoke.out"
  * primed filenodes cache with 3 entries, repo: repo (glob)
  * primed changesets cache with 5 entries, repo: repo (glob)
  * microwave: successfully primed cache, repo: repo (glob)

Finally, check that we can also generate a snapshot to files

  $ mkdir "$TESTTMP/microwave"
  $ quiet microwave_builder local-path "$TESTTMP/microwave"
  $ ls "$TESTTMP/microwave"
  repo0000.microwave_snapshot_v1
