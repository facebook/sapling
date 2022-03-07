# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.
#require slow

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check bookmarks
  $ echo value > "$TESTTMP/value"
  $ mononoke_admin blobstore-upload --key somekey --value-file "$TESTTMP/value"
  * using repo "repo" repoid RepositoryId(0) (glob)
  * writing 6 bytes to blobstore (glob)
  $ mononoke_admin blobstore-fetch somekey --raw-blob "$TESTTMP/fetched_value"
  * using blobstore: Fileblob { base: "$TESTTMP/blobstore/blobs", put_behaviour: Overwrite } (glob)
  $ diff "$TESTTMP/value" "$TESTTMP/fetched_value"
