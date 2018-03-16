  $ . $TESTDIR/library.sh

setup configuration

  $ setup_common_config

  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo "a file content" > a
  $ hg add a
  $ hg ci -ma

  $ cd $TESTTMP
  $ blobimport --blobstore files --linknodes repo-hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

Push with bookmark
  $ cd repo-push
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hgmn push --config extensions.remotenames= --to withbook --create --debug
  running * (glob)
  sending hello command
  sending between command
  remote: 204
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog bundle2=* (glob)
  remote: 1
  pushing rev 11f53bbd855a to destination ssh://user@dummy/repo bookmark withbook
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  1 changesets found
  list of changesets:
  11f53bbd855ac06521a8895bd57e6ce5f46a9980
  sending unbundle command
  bundle2-output-bundle: "HG20", 5 parts total
  bundle2-output-part: "replycaps" 196 bytes payload
  bundle2-output-part: "check:heads" streamed payload
  bundle2-output-part: "changegroup" (params: 1 mandatory) streamed payload
  bundle2-output-part: "pushkey" (params: 4 mandatory) empty payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  server ignored bookmark withbook update
  sending branchmap command
