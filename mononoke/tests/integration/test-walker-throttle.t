# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Drain the healer queue
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "DELETE FROM blobstore_sync_queue";

Base case, check can walk fine
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Check reads throttle
  $ START_SECS=$(/usr/bin/date "+%s")
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore --blobstore-read-qps=5 scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)
  $ END_SECS=$(/usr/bin/date "+%s")
  $ ELAPSED_SECS=$(( "$END_SECS" - "$START_SECS" ))
  $ if [[ "$ELAPSED_SECS" -ge 4 ]]; then echo Took Long Enough Read; else echo "Too short: $ELAPSED_SECS"; fi
  Took Long Enough Read

Delete all data from one side of the multiplex
  $ ls blobstore/0/blobs/* | wc -l
  30
  $ rm blobstore/0/blobs/*

Check writes throttle in Repair mode
  $ START_SECS=$(/usr/bin/date "+%s")
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore --blobstore-write-qps=5 scrub --scrub-blobstore-action=Repair -I deep -q --bookmark master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  27 scrub: blobstore_id BlobstoreId(0) repaired for repo0000.
  1 Final count: (37, 37)
  1 Walked* (glob)
  $ END_SECS=$(/usr/bin/date "+%s")
  $ ELAPSED_SECS=$(( "$END_SECS" - "$START_SECS" ))
  $ if [[ "$ELAPSED_SECS" -ge 4 ]]; then echo Took Long Enough Repair; else echo "Too short: $ELAPSED_SECS"; fi
  Took Long Enough Repair

Check repair happened
  $ ls blobstore/0/blobs/* | wc -l
  27
