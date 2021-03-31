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
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

validate, expecting all valid
  $ mononoke_walker validate -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Seen,Loaded: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:40,3,0; EdgesChecked:9; CheckType:Pass,Fail Total:3,0 HgLinkNodePopulated:3,0

Check that hash validation does not fail when blob is not corrupt
  $ mononoke_walker scrub -I deep -q -b master_bookmark --include-hash-validation-node-type HgFileEnvelope 2>&1 | strip_glog | grep 'failed to validate'
  [1]

Corrupt a blob with content "B"
  $ cd "$TESTTMP/blobstore/blobs"
  $ sed -i 's/B/C/g' blob-repo0000.content.blake2.55662471e2a28db8257939b2f9a2d24e65b46a758bac12914a58f17dcde6905f

Neither scrub nor validate modes notice corrupt blobs
  $ mononoke_walker validate -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Performing check types [HgLinkNodePopulated]
  Seen,Loaded: * (glob)
  Walked* (glob)
  Nodes,Pass,Fail:*,*,0; * (glob)

  $ mononoke_walker scrub -I deep -q -b master_bookmark 2>&1 | strip_glog
  Walking edge types * (glob)
  Walking node types * (glob)
  Seen,Loaded: * (glob)
  Bytes/s,Keys/s,Bytes,Keys;* (glob)
  Walked* (glob)

Now run with hash validation, make sure it fails
  $ mononoke_walker scrub -I deep -q -b master_bookmark --include-hash-validation-node-type HgFileEnvelope 2>&1 | strip_glog | grep 'failed to validate'
      failed to validate filenode hash: expected 35e7525ce3a48913275d7061dd9a867ffef1e34d actual a2e456504a5e61f763f1a0b36a6c247c7541b2b3
