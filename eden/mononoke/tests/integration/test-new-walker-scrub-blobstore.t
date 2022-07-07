# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

setup repo2 so we can try multi-repo
  $ hginit_treemanifest repo2-hg
  $ cd repo2-hg
  $ mkcommit X
  $ hg bookmark master_bookmark -r tip
  $ cd ..
  $ MULTIPLEXED=1 REPOID=2 setup_mononoke_repo_config repo2 blobstore
  $ cd ..
  $ REPOID=2 blobimport repo2-hg/.hg repo2 --exclude-derived-data-type=filenodes

Drain the healer queue
  $ sqlite3 "$TESTTMP/blobstore_sync_queue/sqlite_dbs" "DELETE FROM blobstore_sync_queue";

Base case, check can walk fine, one repo
  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Seen,Loaded: 40,40
  Bytes/s,* (glob)
  Walked* (glob)

Check that multi repo runs for all repos specified
  $ mononoke_walker --repo-name repo2 scrub -I deep -q -b master_bookmark 2>&1 | strip_glog > multi_repo.log
  $ grep repo multi_repo.log
  Walking repos ["repo", "repo2"]* (glob)
  Walking edge types *, repo: repo* (glob)
  Walking node types *, repo: repo* (glob)
  Walking edge types *, repo: repo2* (glob)
  Walking node types *, repo: repo2* (glob)
  Seen,Loaded: 8,8, repo: repo2* (glob)
  Bytes/s,*, repo: repo2* (glob)
  Walked*, repo: repo2* (glob)
  Seen,Loaded: 40,40, repo: repo* (glob)
  Bytes/s,*, repo: repo* (glob)
  Walked*, repo: repo* (glob)

Delete all data from one side of the multiplex
  $ ls blobstore/0/blobs/* | wc -l
  40
  $ rm blobstore/0/blobs/*

Check fails on only the deleted side
  $ mononoke_walker -L graph scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | strip_glog
  Error: Could not step to OutgoingEdge { label: BookmarkToChangeset, target: Changeset(ChangesetKey { inner: ChangesetId(Blake2(c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd)), filenode_known_derived: false })* (glob)
  * (glob)
  Caused by:
      changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd is missing

Check can walk fine on the only remaining side
  $ mononoke_walker -L graph scrub -q --inner-blobstore-id=1 -I deep -b master_bookmark 2>&1 | strip_glog
  Seen,Loaded: 40,40
  Bytes/s,Keys/s,Bytes,Keys; Delta */s,*/s,2168,30,0s; Run */s,*/s,2168,30,*s; Type:Raw,Compressed AliasContentMapping:333,9 BonsaiHgMapping:281,3 Bookmark:0,0 Changeset:277,3 FileContent:12,3 FileContentMetadata:351,3 HgBonsaiMapping:0,0 HgChangeset:281,3 HgChangesetViaBonsai:0,0 HgFileEnvelope:189,3 HgFileNode:0,0 HgManifest:444,3* (glob)


Check can walk fine on the multiplex remaining side
  $ mononoke_walker -l loaded scrub -q -I deep -b master_bookmark 2>&1 | strip_glog
  Seen,Loaded: 40,40

Check can walk fine on the multiplex with scrub-blobstore enabled in ReportOnly mode, should log the scrub repairs needed
  $ mononoke_walker -l loaded --blobstore-scrub-action=ReportOnly --scuba-log-file scuba-reportonly.json scrub -q -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  * scrub: blobstore_id BlobstoreId(0) not repaired for repo0000. (glob)
  1 Seen,Loaded: 40,40

Check scuba data
Note - we might get duplicate reports, we just expect that there should not be a lot of them
  $ LINES="$(wc -l < scuba-reportonly.json)"
  $ [[ $LINES -lt 50 ]]
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .repo, .walk_type, .ctime ] | @csv' < scuba-reportonly.json | sort | uniq
  1,"scrub_repair","repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgchangeset.sha1.112478962961147124edd43549aedd1a335e44bf","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgmanifest.sha1.41b34f08c1356f6ad068e9ab9b43d984245111aa","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgmanifest.sha1.7c9b4fd8b49377e2fead2e9610bb8db910a98c53","repo","scrub",1* (glob)
  1,"scrub_repair","repo0000.hgmanifest.sha1.eb79886383871977bccdb3000c275a279f0d4c99","repo","scrub",1* (glob)

Check that walking with a grace period does not report the errors as the keys are too new
  $ mononoke_walker -l loaded --blobstore-scrub-grace=3600 --blobstore-scrub-action=ReportOnly --scuba-log-file scuba-reportonly-grace.json scrub -q -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. not repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  1 Seen,Loaded: 40,40
  $ LINES="$(wc -l < scuba-reportonly-grace.json)"
  $ [[ $LINES -lt 1 ]]

Check can walk fine on the multiplex with scrub-blobstore enabled in Repair mode, should also log the scrub repairs done
  $ mononoke_walker -l loaded --blobstore-scrub-action=Repair --scuba-log-file scuba-repair.json scrub -q -I deep -b master_bookmark 2>&1 | strip_glog | sed -re 's/^(scrub: blobstore_id BlobstoreId.0. repaired for repo0000.).*/\1/' | uniq -c | sed 's/^ *//'
  * scrub: blobstore_id BlobstoreId(0) repaired for repo0000. (glob)
  1 Seen,Loaded: 40,40

Check scuba data
Note - we might get duplicate repairs, we just expect that there should not be a lot of them
  $ LINES="$(wc -l < scuba-repair.json)"
  $ [[ $LINES -lt 50 ]]
  $ jq -r '.int * .normal | [ .check_fail, .check_type, .node_key, .repo, .walk_type, .ctime ] | @csv' < scuba-repair.json | sort | uniq
  0,"scrub_repair","repo0000.alias.gitsha1.7371f47a6f8bd23a8fa1a8b2a9479cdd76380e54","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.gitsha1.8c7e5a667f1b771847fe88c01c3de34413a1b220","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.gitsha1.96d80cd6c4e7158dbebd0849f4fb7ce513e5828c","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha1.32096c2e0eff33d844ee6d675407ace18289357d","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha1.6dcd4ce23d88e2ee9568ba546c007c63d9131c1b","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha1.ae4f281df5a5d0ff3cad6371f76d5c29b6d953ec","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha256.559aead08264d5795d3909718cdd05abd49572e84fe55590eef31a88a08fdffd","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha256.6b23c0d5f35d1b11f9b683f0b0a617355deb11277d91ae091d399c655b87940d","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.alias.sha256.df7e70e5021544f4834bbee64a9e3789febc4be81470df629cad6ddb03320a5c","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.changeset.blake2.459f16ae564c501cb408c1e5b60fc98a1e8b8e97b9409c7520658bfa1577fb66","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.changeset.blake2.9feb8ddd3e8eddcfa3a4913b57df7842bedf84b8ea3b7b3fcb14c6424aa81fec","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.changeset.blake2.c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content_metadata.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content_metadata.blake2.896ad5879a5df0403bfc93fc96507ad9c93b31b11f3d0fa05445da7918241e5d","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.content_metadata.blake2.eb56488e97bb4cf5eb17f05357b80108a4a71f6c3bab52dfcaec07161d105ec9","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgchangeset.sha1.112478962961147124edd43549aedd1a335e44bf","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgfilenode.sha1.005d992c5dcf32993668f7cede29d296c494a5d9","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgfilenode.sha1.a2e456504a5e61f763f1a0b36a6c247c7541b2b3","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgmanifest.sha1.41b34f08c1356f6ad068e9ab9b43d984245111aa","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgmanifest.sha1.7c9b4fd8b49377e2fead2e9610bb8db910a98c53","repo","scrub",1* (glob)
  0,"scrub_repair","repo0000.hgmanifest.sha1.eb79886383871977bccdb3000c275a279f0d4c99","repo","scrub",1* (glob)

Check that all is repaired by running on only the deleted side
  $ mononoke_walker -l loaded scrub -q --inner-blobstore-id=0 -I deep -b master_bookmark 2>&1 | strip_glog
  Seen,Loaded: 40,40

Check the files after restore.  The blobstore filenode_lookup representation is currently not traversed, so remains as a difference
  $ ls blobstore/0/blobs/* | wc -l
  27
  $ diff -ur blobstore/0/blobs/ blobstore/1/blobs/ | grep -E -v blob-repo0002
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.61585a6b75335f6ec9540101b7147908564f2699dcad59134fdf23cb086787ad
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.9915e555ad3fed014aa36a4e48549c1130fddffc7660589f42af5f0520f1118e
  Only in blobstore/1/blobs/: blob-repo0000.filenode_lookup.a0377040953a1a3762b7c59cb526797c1afd7ae6fcebb4d11e3c9186a56edb4e
