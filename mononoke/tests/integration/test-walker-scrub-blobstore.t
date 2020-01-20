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
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --scrub-blobstore-action=ReportOnly -I deep -q --bookmark master_bookmark --scuba-log-file scuba-reportonly.json 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  27 scrub: blobstore_id BlobstoreId(0) not repaired for repo0000.
  1 Final count: (37, 37)
  1 Walked* (glob)

Check scuba data
  $ wc -l < scuba-reportonly.json
  27
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .repo, .walk_type ] | @csv' < scuba-reportonly.json | sort
  1,"scrub_repair","repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54","repo","scrub"
  1,"scrub_repair","repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220","repo","scrub"
  1,"scrub_repair","repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d","repo","scrub"
  1,"scrub_repair","repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c","repo","scrub"
  1,"scrub_repair","repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66","repo","scrub"
  1,"scrub_repair","repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec","repo","scrub"
  1,"scrub_repair","repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","repo","scrub"
  1,"scrub_repair","repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub"
  1,"scrub_repair","repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub"
  1,"scrub_repair","repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub"
  1,"scrub_repair","repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub"
  1,"scrub_repair","repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub"
  1,"scrub_repair","repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub"
  1,"scrub_repair","repo0000.hgchangeset.sha1.112478962961147124edd43549aedd1a335e44bf","repo","scrub"
  1,"scrub_repair","repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b","repo","scrub"
  1,"scrub_repair","repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0","repo","scrub"
  1,"scrub_repair","repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9","repo","scrub"
  1,"scrub_repair","repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","repo","scrub"
  1,"scrub_repair","repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3","repo","scrub"
  1,"scrub_repair","repo0000.hgmanifest.sha1.41b34f08c1356f6ad068e9ab9b43d984245111aa","repo","scrub"
  1,"scrub_repair","repo0000.hgmanifest.sha1.7c9b4fd8b49377e2fead2e9610bb8db910a98c53","repo","scrub"
  1,"scrub_repair","repo0000.hgmanifest.sha1.eb79886383871977bccdb3000c275a279f0d4c99","repo","scrub"

Check can walk fine on the multiplex with scrub-blobstore enabled in Repair mode, should also log the scrub repairs done
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore scrub --scrub-blobstore-action=Repair -I deep -q --bookmark master_bookmark --scuba-log-file scuba-repair.json 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Walking roots * (glob)
  1 Walking edge types * (glob)
  1 Walking node types * (glob)
  27 scrub: blobstore_id BlobstoreId(0) repaired for repo0000.
  1 Final count: (37, 37)
  1 Walked* (glob)

Check scuba data
  $ wc -l < scuba-repair.json
  27
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .repo, .walk_type ] | @csv' < scuba-repair.json | sort
  0,"scrub_repair","repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54","repo","scrub"
  0,"scrub_repair","repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220","repo","scrub"
  0,"scrub_repair","repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d","repo","scrub"
  0,"scrub_repair","repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c","repo","scrub"
  0,"scrub_repair","repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66","repo","scrub"
  0,"scrub_repair","repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec","repo","scrub"
  0,"scrub_repair","repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","repo","scrub"
  0,"scrub_repair","repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub"
  0,"scrub_repair","repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub"
  0,"scrub_repair","repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub"
  0,"scrub_repair","repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub"
  0,"scrub_repair","repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub"
  0,"scrub_repair","repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub"
  0,"scrub_repair","repo0000.hgchangeset.sha1.112478962961147124edd43549aedd1a335e44bf","repo","scrub"
  0,"scrub_repair","repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b","repo","scrub"
  0,"scrub_repair","repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0","repo","scrub"
  0,"scrub_repair","repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9","repo","scrub"
  0,"scrub_repair","repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","repo","scrub"
  0,"scrub_repair","repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3","repo","scrub"
  0,"scrub_repair","repo0000.hgmanifest.sha1.41b34f08c1356f6ad068e9ab9b43d984245111aa","repo","scrub"
  0,"scrub_repair","repo0000.hgmanifest.sha1.7c9b4fd8b49377e2fead2e9610bb8db910a98c53","repo","scrub"
  0,"scrub_repair","repo0000.hgmanifest.sha1.eb79886383871977bccdb3000c275a279f0d4c99","repo","scrub"

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
