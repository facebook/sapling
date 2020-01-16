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

Base case, check can walk fine
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Delete all data from one side of the multiplex
  $ ls blobstore/0/blobs/* | wc -l
  30
  $ rm blobstore/0/blobs/*

Check fails on only the deleted side
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --inner-blobstore-id=0 -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Execution error: Could not step to OutgoingEdge { label: BookmarkToBonsaiChangeset, target: BonsaiChangeset(ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd))) }
  * (glob)
  Caused by:
      Blob is missing: changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Error: Execution failed

Check can walk fine on the only remaining side
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --inner-blobstore-id=1 -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Check can walk fine on the multiplex remaining side
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Check can walk fine on the multiplex with scrub-blobstore enabled in ReportOnly mode, should log the scrub repairs needed
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --scrub-blobstore-action=ReportOnly -I deep -q --bookmark master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  27 scrub: blobstore_id BlobstoreId(0) not repaired for repo0000.
  1 Final count: (37, 37)
  1 Walked* (glob)

Check can walk fine on the multiplex with scrub-blobstore enabled in Repair mode, should also log the scrub repairs done
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --scrub-blobstore-action=Repair -I deep -q --bookmark master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  27 scrub: blobstore_id BlobstoreId(0) repaired for repo0000.
  1 Final count: (37, 37)
  1 Walked* (glob)

Check that all is repaired by running on only the deleted side
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --inner-blobstore-id=0 -I deep -q --bookmark master_bookmark 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: (37, 37)
  Walked* (glob)

Check the files after restore.  The blobstore filenode_lookup representation is currently not traversed, so remains as a difference
  $ ls blobstore/0/blobs/* | wc -l
  27
  $ diff -ur blobstore/0/blobs/ blobstore/1/blobs/
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.61585a6b75335f6ec9540101b7147908564f2699dcad59134fdf23cb086787ad
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.9915e555ad3fed014aa36a4e48549c1130fddffc7660589f42af5f0520f1118e
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.a0377040953a1a3762b7c59cb526797c1afd7ae6fcebb4d11e3c9186a56edb4e
  [1]
