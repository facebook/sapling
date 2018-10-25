  $ . $TESTDIR/library.sh

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

# Attempt to make the max possible file len of Upper Case letter and underscores
# hg accept such a file, as 253 is max length
# 253 because: 253 + len(".i") = 255 (max filename in UNIX system)
  $ for ((i=1;i<=100;i++)); do
  >     FILENAME=$(cat /dev/urandom | tr -dc 'A-H_' | fold -w 253 | head -n 1)
  >     echo s > $FILENAME
  >     hg commit -Aqm "Capitals and Underscores commit"$i
  > done

# 253 because: 253 + len(".i") = 255 (max filename in UNIX system)
  $ for ((i=1;i<=100;i++)); do
  >     FILENAME=$(cat /dev/urandom | tr -dc 'a-hA-H_' | fold -w 253 | head -n 1)
  >     echo s > $FILENAME
  >     hg commit -Aqm "Small, Capital and Underscores commit"$i
  > done

  $ cd $TESTTMP

  $ blobimport rocksdb repo-hg/.hg repo
