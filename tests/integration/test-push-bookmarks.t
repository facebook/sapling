  $ . $TESTDIR/library.sh

setup configuration
  $ export ONLY_FAST_FORWARD_BOOKMARK="master_bookmark"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX="ffonly.*"
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
  pushing rev 66b9c137712a to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  updating bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0:0e7ec5675652
     default/withbook          2:66b9c137712a

Try non fastforward moves (backwards and across branches)
  $ cd ../repo-push
  $ hg update -q master_bookmark
  $ echo other_commit > other_commit && hg -q addremove && hg ci -m other_commit
  $ hgmn push
  pushing to ssh://user@dummy/repo
  searching for changes
  updating bookmark master_bookmark
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to master_bookmark
  pushing rev 0e7ec5675652 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Non fastforward bookmark move",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     Non fastforward bookmark move
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 66b9c137712a --to master_bookmark
  pushing rev 66b9c137712a to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Non fastforward bookmark move",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     Non fastforward bookmark move
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to withbook
  pushing rev 0e7ec5675652 to destination ssh://user@dummy/repo bookmark withbook
  searching for changes
  no changes found
  updating bookmark withbook
  [1]
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   3:a075b5221b92
     default/withbook          0:0e7ec5675652

Try non fastfoward moves on regex bookmark
  $ hgmn push -r a075b5221b92 --to ffonly_bookmark --create -q
  [1]
  $ hgmn push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to ffonly_bookmark
  pushing rev 0e7ec5675652 to destination ssh://user@dummy/repo bookmark ffonly_bookmark
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Non fastforward bookmark move",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     Non fastforward bookmark move
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Try to delete master
  $ cd ../repo-push
  $ hgmn push --delete master_bookmark
  pushing to ssh://user@dummy/repo
  searching for changes
  no changes found
  remote: Command failed
  remote:   Error:
  remote:     bundle2_resolver error
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Deletion of bookmark master_bookmark is forbidden.",
  remote:     }
  remote:   Caused by:
  remote:     While updating Bookmarks
  remote:   Caused by:
  remote:     Deletion of bookmark master_bookmark is forbidden.
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

Delete the bookmark
  $ hgmn push --delete withbook
  pushing to ssh://user@dummy/repo
  searching for changes
  no changes found
  deleting remote bookmark withbook
  [1]
  $ cd ../repo-pull
  $ hgmn pull -q
  devel-warn: applied empty changegroup * (glob)
  $ hg book --remote
     default/ffonly_bookmark   3:a075b5221b92
     default/master_bookmark   3:a075b5221b92
