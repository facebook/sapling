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
  $ blobimport repo-hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull

start mononoke

  $ mononoke -P $TESTTMP/mononoke-config -B test-config
  $ wait_for_mononoke $TESTTMP/repo

Push with bookmark
  $ cd repo-push
  $ enableextension remotenames
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hgmn push --to withbook --create
  pushing rev 11f53bbd855a to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  exporting bookmark withbook

Pull the bookmark
  $ cd ../repo-pull
  $ enableextension remotenames

  $ hgmn pull -q
  $ hg book --remote
     default/withbook          1:11f53bbd855a

Update the bookmark
  $ cd ../repo-push
  $ echo update > update && hg addremove && hg ci -m update
  adding update
  $ hgmn push --to withbook
  pushing rev 66b9c137712a to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  remote has heads that are not known locally
  updating bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/withbook          2:66b9c137712a

TODO(stash): Deleting the bookmark doesn't work yet
  $ cd ../repo-push
  $ hgmn push --delete withbook > /dev/null 2>&1
  [255]
