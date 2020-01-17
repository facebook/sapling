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

Check bookmarks
  $ mononoke_admin bookmarks log master_bookmark 2>&1 | grep master_bookmark
  (master_bookmark) 26805aba1e600a82e93661149f2313866a221a7b blobimport * (glob)

  $ mononoke_admin bookmarks set another_bookmark 26805aba1e600a82e93661149f2313866a221a7b 2>/dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null | sort
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b
  master_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

  $ mononoke_admin bookmarks delete master_bookmark 2> /dev/null

  $ mononoke_admin bookmarks list --kind publishing 2> /dev/null
  another_bookmark	c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd	26805aba1e600a82e93661149f2313866a221a7b

Check blobstore-fetch, normal
  $ mononoke_admin blobstore-fetch changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: MultiplexedBlobstore* (glob)
  Some(BlobstoreBytes(* (glob)

Check blobstore-fetch, with scrub actions
  $ ls blobstore/1/blobs | wc -l
  30
  $ rm blobstore/1/blobs/*changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd*
  $ ls blobstore/1/blobs | wc -l
  29

  $ mononoke_admin blobstore-fetch --scrub-blobstore-action=ReportOnly changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) not repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreBytes(* (glob)
  $ ls blobstore/1/blobs | wc -l
  29

  $ mononoke_admin blobstore-fetch --scrub-blobstore-action=Repair changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd 2>&1 | strip_glog
  using blobstore: ScrubBlobstore* (glob)
  scrub: blobstore_id BlobstoreId(1) repaired for repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd
  Some(BlobstoreBytes(* (glob)
  $ ls blobstore/1/blobs | wc -l
  30
