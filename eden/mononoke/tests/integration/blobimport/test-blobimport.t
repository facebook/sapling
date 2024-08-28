# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

# setup repo, usefncache flag for forcing algo encoding run
  $ hginit_treemanifest repo --config format.usefncache=False
  $ cd repo

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
  $ UNDERSCORES=`printf '_%.0s' {1..122}`
  $ CAPITALS=`printf 'A%.0s' {1..131}`
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

# The repo only has draft commits, since we didn't create a bookmark
  $ hg log -G -T "{node|short} {phase} '{desc}' {bookmarks} {remotenames}"
  @  adf59c870cc7 draft 'underscores, capitals, lowercase'
  │
  o  ef4315833609 draft 'underscores, capitals'
  │
  o  ae43465b9c0f draft 'Encoding results in 255 symbols, go to second type'
  │
  o  646c458a63f3 draft 'Encoding results in 253 symbols'
  │
  o  31197a58ae5f draft 'very long name, after encoding capitals len(str) > 255'
  

  $ setup_mononoke_config
  $ cd $TESTTMP
  $ blobimport --log repo/.hg repo --find-already-imported-rev-only
  * using repo "repo" repoid RepositoryId(0) (glob)
  * didn't import any commits (glob)
  $ blobimport --log repo/.hg repo --commits-limit 1
  * using repo "repo" repoid RepositoryId(0) (glob)
  * inserted commits # 0 (glob)
  * Deriving data for: ["filenodes"] (glob)
  * finished uploading changesets, globalrevs and deriving data (glob)
  * latest imported revision 0 (glob)
  $ blobimport --log repo/.hg repo --find-already-imported-rev-only
  * using repo "repo" repoid RepositoryId(0) (glob)
  * latest imported revision 0 (glob)
  $ blobimport repo/.hg repo
  $ blobimport --log repo/.hg repo --find-already-imported-rev-only
  * using repo "repo" repoid RepositoryId(0) (glob)
  * latest imported revision 4 (glob)
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select * from mutable_counters";
  0|highest-imported-gen-num|5
# Show that all changesets in the repo are public after running blobimport.
# This is a bug, but we don't necessarily care to fix it as blobimport is deprecated.
# Instead, we should stop relying on blobimport for integration tests in favour of modern
# alternatives.
# In the meantime, document the surprising behaviour in this test for posterity.
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select repo_id,hex(cs_id),phase from phases";
  0|614D09D0831FBCD064137FF332C31A2B346740DA8CA27E79977A4FCA5D856FB6|Public
  0|6DB99A6258A7712B00FDD999FCDF5C95DBE6A2C20A878901CA945D361BFF5E95|Public
  0|ECA64DEE255CE6C77B60CDEE900CD19A04761DF39FEEBF7CDC1F0D9515A2BC41|Public
  0|710015794687E2DF01E847BF0258D5B38E44489C5B6307BCCC39B35DBA8C22AD|Public
  0|DEEBA4055695BF253E33685105C9D43864378063CA4253250BF01CF6D2947547|Public
