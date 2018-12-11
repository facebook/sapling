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

setup master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport

  $ cd $TESTTMP
  $ blobimport repo-hg/.hg repo

setup two repos: one will be used to push from, another will be used
to pull these pushed commits

  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

Push with bookmark
  $ cd repo-push
  $ enableextension remotenames
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hgmn push --to withbook --create
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 11f53bbd855a to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  exporting bookmark withbook

Pull the bookmark
  $ cd ../repo-pull
  $ enableextension remotenames

  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0:0e7ec5675652
     default/withbook          1:11f53bbd855a

Update the bookmark
  $ cd ../repo-push
  $ echo update > update && hg addremove && hg ci -m update
  adding update
  $ hgmn push --to withbook
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 66b9c137712a to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  updating bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0:0e7ec5675652
     default/withbook          2:66b9c137712a

Delete the bookmark
  $ cd ../repo-push
  $ hgmn push --delete withbook
  pushing to ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  no changes found
  deleting remote bookmark withbook
  [1]
  $ cd ../repo-pull
  $ hgmn pull -q
  devel-warn: applied empty changegroup * (glob)
  $ hg book --remote
     default/master_bookmark   0:0e7ec5675652
