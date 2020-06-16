# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ MULTIPLEXED=1 PACK_BLOB=0 default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Check the stores have expected counts
  $ ls blobstore/0/blobs/ | wc -l
  30
  $ ls blobstore/1/blobs/ | wc -l
  30

Check that the store sizes are different due to the packblob wrappers on store 0
  $ PACKED=$(du -s --bytes blobstore/0/blobs/ | cut -f1); UNPACKED=$(du -s --bytes blobstore/1/blobs/ | cut -f1)
  $ if [[ "$PACKED" -le "$UNPACKED" ]]; then echo "expected packed $PACKED to be larger than unpacked $UNPACKED"; fi
