# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Erase the sqllites and blobstore_sync_queue
  $ rm -rf "$TESTTMP/monsql/sqlite_dbs" "$TESTTMP/blobstore"

blobimport them into Mononoke storage again, but with write failures on one side
  $ blobimport repo-hg/.hg repo --blobstore-write-chaos-rate=1

Check that healer queue has successful items
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM blobstore_sync_queue";
  60

Check that scrub doesnt report issues despite one store being missing, as the entries needed are on the queue and less than N minutes old
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --scrub-blobstore-action=ReportOnly -I deep -q --bookmark master_bookmark --scuba-log-file scuba-reportonly.json 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  1 Final count: (37, 37)
  1 Walked* (glob)
