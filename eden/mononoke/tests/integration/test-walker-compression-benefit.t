# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ default_setup_blobimport "blob_files"
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  |
  o  B [draft;rev=1;112478962961]
  |
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

compression-benefit, not expecting any compression from the tiny test files
  $ mononoke_walker --storage-id=blobstore --readonly-storage --cachelib-only-blobstore compression-benefit -q --bookmark master_bookmark --sample-rate 1 2>&1 | strip_glog
  Walking roots * (glob)
  Walking edge types * (glob)
  Walking node types * (glob)
  Final count: * (glob)
  * Total: SizingStats { raw: 3, compressed: 3 },000% * (glob)
