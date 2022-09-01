# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ . "${TEST_FIXTURES}/library.sh"
  $ BLOB_TYPE="blob_files" default_setup_blobimport
  hg repo
  o  C [draft;rev=2;26805aba1e60]
  │
  o  B [draft;rev=1;112478962961]
  │
  o  A [draft;rev=0;426bada5c675]
  $
  blobimporting

Derive data
  $ backfill_derived_data single c3384961b16276f2db77df9d7c874bbe981cf0525bd6f84a502f919044f2dabd --all-types 2>&1 | grep 'derived .* in' | wc -l
  10
  $ cd $TESTTMP/repo-hg
  $ hg log -r ':' -T '{node}\n' > "$TESTTMP/hashes.txt"
  $ cat "$TESTTMP/hashes.txt"
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf
  26805aba1e600a82e93661149f2313866a221a7b
  $ backfill_derived_data benchmark --input-file "$TESTTMP/hashes.txt" --all-types --backfill --batch-size 2 --parallel 2>&1 | grep 'took'
  Building derive graph took * (glob)
  Derivation took * (glob)
