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

Base case, check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  30
  $ ls blobstore/1/blobs/ | wc -l
  30
  $ ls blobstore/2/blobs/ | wc -l
  30

Check that healer queue has all items
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM blobstore_sync_queue";
  90

Erase the sqllites and blobstore_sync_queue
  $ rm -rf "$TESTTMP/monsql/sqlite_dbs" "$TESTTMP/blobstore/"*/blobs/*

blobimport them into Mononoke storage again, but with failures on one side
  $ blobimport repo-hg/.hg repo --blobstore-write-chaos-rate=1

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  0
  $ ls blobstore/1/blobs/ | wc -l
  30
  $ ls blobstore/2/blobs/ | wc -l
  30

Check that healer queue has successful items
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select count(*) FROM blobstore_sync_queue";
  60
