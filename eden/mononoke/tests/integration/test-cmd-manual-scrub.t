# Copyright (c) Facebook, Inc. and its affiliates.
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
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Run a heal
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 > /dev/null

Failure time - this key will not exist
  $ manual_scrub --storage-config-name blobstore <<EOF
  > fake-key
  > EOF
  Error: Scrub failed
  
  Caused by:
      Key fake-key is missing
  [1]

Success time - these keys will exist and be scrubbed
  $ manual_scrub --storage-config-name blobstore <<EOF
  > repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  > repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  > repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d
  > EOF
  repo0000.hgchangeset.sha1.26805aba1e600a82e93661149f2313866a221a7b
  repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f
  repo0000.hgfilenode.sha1.35e7525ce3a48913275d7061dd9a867ffef1e34d

Demostrate that a key exists
  $ ls "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
  $TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
Delete it
  $ rm "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
Scrub restores it
  $ manual_scrub --storage-config-name blobstore <<EOF
  > repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  > EOF
  * scrub: blobstore_id BlobstoreId(0) repaired for repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0 (glob)
  repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
Run a heal and demonstrate that it's back
  $ mononoke_blobstore_healer -q --iteration-limit=1 --heal-min-age-secs=0 --storage-id=blobstore --sync-queue-limit=100 2>&1 > /dev/null
  $ ls "$TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0"
  $TESTTMP/blobstore/0/blobs/blob-repo0000.hgchangeset.sha1.426bada5c67598ca65036d57d9e4b64b0c1ce7a0
