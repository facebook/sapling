# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usefncache flag for forcing algo encoding run
  $ hg init repo-hg --config format.usefncache=False

# Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > [treemanifest]
  > server=True
  > EOF

# From blobimport fail real case
  $ DIR="data/scm/www/Hhg/store/data/flib/site/web/ads/rtb_neko/web_request/__tests__/codegen/__snapshots__"
  $ FILENAME=$DIR"/AdNetworkInstreamVideoRequestParametersIntegrationTestWithSnapshot-testIsURLReviewedByHumanForANReservedBuying_with_data_set__Non-empty_set_without_UNCLASSIFIED_ID_categories_returned_true_since_it_means_the_review_result_is_bad__but_reviewed_.json"
  $ mkdir -p $DIR
  $ echo s > $FILENAME
  $ hg commit -Aqm "very long name, after encoding capitals len(str) > 255"

# Encoding results in 253 symbols (1 + 127 * 2)
  $ FILENAME="aAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  $ echo s > $FILENAME
  $ hg commit -Aqm "Encoding results in 253 symbols"

# Encoding results in 255 symbols (1 + 127 * 2)
  $ FILENAME="aAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"
  $ echo s > $FILENAME
  $ hg commit -Aqm "Encoding results in 255 symbols, go to second type"

# Capitals and underscores in a filename. Total len of the filename is 253
# 253 because: 253 + len(".i") = 255 (max filename in UNIX system)
  $ UNDERSCORES=`printf '_%.0s' {1..123}`
  $ CAPITALS=`printf 'A%.0s' {1..130}`
  $ echo s > "$UNDERSCORES$CAPITALS"
  $ hg commit -Aqm "underscores, capitals"

# Capitals, lowercase and underscores in a filename. Total len of the filename
# is 253
# 253 because: 253 + len(".i") = 255 (max filename in UNIX system)
  $ UNDERSCORES=`printf '_%.0s' {1..123}`
  $ CAPITALS=`printf 'A%.0s' {1..100}`
  $ LOWERCASE=`printf 'a%.0s' {1..30}`
  $ echo s > "$UNDERSCORES$CAPITALS$LOWERCASE"
  $ hg commit -Aqm "underscores, capitals, lowercase"

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo
