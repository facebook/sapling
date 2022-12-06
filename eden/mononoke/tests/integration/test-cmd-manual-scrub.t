# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

setup
  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=2 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Run a heal
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 > /dev/null

Failure time - this key will not exist
  $ echo fake-key | manual_scrub --storage-config-name blobstore --checkpoint-key-file=checkpoint.txt --error-keys-output errors --missing-keys-output missing --success-keys-output success 2>&1 | strip_glog
  Scrubbing blobstore: ScrubBlobstore[Normal [(BlobstoreId(0), "Fileblob"), (BlobstoreId(1), "Fileblob"), (BlobstoreId(2), "Fileblob")], write only []]
  period, rate/s, seconds, success, missing, error, total, skipped, bytes, bytes/s
  run, *, *, 0, 1, 0, 1, 0, * (glob)
  delta, *, *, 0, 1, 0, 1, 0, * (glob)
  $ wc -l success missing errors
  0 success
  1 missing
  0 errors
  1 total
  $ cat missing
  fake-key
  $ [ ! -r checkpoint.txt ] || echo "not expecting checkpoint as no successes"

Success time - these keys will exist and be scrubbed
  $ manual_scrub --storage-config-name blobstore --quiet --scheduled-max=1 --checkpoint-key-file=checkpoint.txt --error-keys-output errors --missing-keys-output missing --success-keys-output success <<EOF 2>&1 | strip_glog
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  > EOF
  checkpointed repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  $ sort < success
  repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  $ wc -l success missing errors
    2 success
    0 missing
    0 errors
    2 total
  $ cat checkpoint.txt
  repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b (no-eol)

Continue from checkpoint
  $ manual_scrub --storage-config-name blobstore --scheduled-max=1 --checkpoint-key-file=checkpoint.txt --error-keys-output errors --missing-keys-output missing --success-keys-output success <<EOF 2>&1 | strip_glog
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  > repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  > EOF
  Scrubbing blobstore: ScrubBlobstore[Normal [(BlobstoreId(0), "Fileblob"), (BlobstoreId(1), "Fileblob"), (BlobstoreId(2), "Fileblob")], write only []]
  period, rate/s, seconds, success, missing, error, total, skipped, bytes, bytes/s
  run, *, *, 1, 0, 0, 1, 2, * (glob)
  delta, *, *, 1, 0, 0, 1, 2, * (glob)
  checkpointed repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  $ sort < success
  repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  $ wc -l success missing errors
   1 success
   0 missing
   0 errors
   1 total
  $ cat checkpoint.txt
  repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d (no-eol)

Do same run with compressed key output
  $ manual_scrub --storage-config-name blobstore --quiet --keys-zstd-level=9 --error-keys-output errors --missing-keys-output missing --success-keys-output success <<EOF
  > repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  > EOF
  $ cat success | zstd -d | wc -l
  3

Do same run without specifing the optional success file when checkpointing
  $ rm success
  $ manual_scrub --storage-config-name blobstore --scheduled-max=1 --checkpoint-key-file=checkpoint2.txt --quiet --error-keys-output errors --missing-keys-output missing <<EOF 2>&1 | strip_glog
  > repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  > EOF
  checkpointed repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d

Demostrate that a key exists
  $ ls "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
  $TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0

Delete it
  $ rm "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"

Check that healer queue is empty
  $ read_blobstore_sync_queue_size
  0

Scrub restores it
  $ manual_scrub --storage-config-name blobstore --quiet --error-keys-output errors --missing-keys-output missing --success-keys-output success <<EOF
  > repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  > EOF
  * scrub: blobstore_id BlobstoreId(0) repaired for repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0 (glob)
  $ wc -l success missing errors
   1 success
   0 missing
   0 errors
   1 total
  $ cat success
  repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0

Demonstrate its back
  $ ls "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
  $TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0

Check that healer queue is empty
  $ read_blobstore_sync_queue_size
  0

Damage the contents of blobstore 0 to demonstrate error handling
The error will group blobstores 1 and 2 together, and leave blobstore 0 in its own group
  $ echo "foo" > "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
  $ manual_scrub --storage-config-name blobstore --quiet --error-keys-output errors --missing-keys-output missing --success-keys-output success <<EOF
  > repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  > EOF
  Error: Scrubbing key repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  * (glob)
  Caused by:
      Different blobstores have different values for this item: [{*}, {*}] are grouped by content, {} do not have (glob)
  $ wc -l success missing errors
   0 success
   0 missing
   1 errors
   1 total
  $ cat errors
  repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
