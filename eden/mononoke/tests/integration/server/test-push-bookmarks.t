# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ export ONLY_FAST_FORWARD_BOOKMARK="master_bookmark"
  $ export ONLY_FAST_FORWARD_BOOKMARK_REGEX="ffonly.*"
  $ setconfig push.edenapi=true
  $ ENABLE_API_WRITES=1 setup_common_config
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

  $ start_and_wait_for_mononoke_server
Push with bookmark
  $ cd repo-push
  $ enableextension remotenames
  $ echo withbook > withbook && hg addremove && hg ci -m withbook
  adding withbook
  $ hgedenapi push --to withbook --create
  pushing rev 11f53bbd855a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark withbook
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  creating remote bookmark withbook

Pull the bookmark
  $ cd ../repo-pull
  $ enableextension remotenames

  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0e7ec5675652
     default/withbook          11f53bbd855a

Update the bookmark
  $ cd ../repo-push
  $ echo update > update && hg addremove && hg ci -m update
  adding update
  $ hgedenapi push --to withbook
  pushing rev 66b9c137712a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark withbook
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  pushrebasing stack (11f53bbd855a, 66b9c137712a] (1 commit) to remote bookmark withbook
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated remote bookmark withbook to 66b9c137712a
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   0e7ec5675652
     default/withbook          66b9c137712a

Try non fastforward moves (backwards and across branches)
  $ cd ../repo-push
  $ hg update -q master_bookmark
  $ echo other_commit > other_commit && hg -q addremove && hg ci -m other_commit
  $ hgedenapi push
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  updating bookmark master_bookmark
  $ hgedenapi push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to master_bookmark
  pushing rev 0e7ec5675652 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from a075b5221b92 to 0e7ec5675652
  abort: server error: invalid request: Non fast-forward bookmark move of 'master_bookmark' from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  [255]
  $ hgedenapi push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 66b9c137712a --to master_bookmark
  pushing rev 66b9c137712a to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark master_bookmark
  moving remote bookmark master_bookmark from a075b5221b92 to 66b9c137712a
  abort: server error: invalid request: Non fast-forward bookmark move of 'master_bookmark' from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to b1a2d38c877a990517a50f9bf928770dd7d3b5b9dbef412d7dafd2ccd2ede0fb
  [255]
  $ hgedenapi push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to withbook
  pushing rev 0e7ec5675652 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark withbook
  moving remote bookmark withbook from 66b9c137712a to 0e7ec5675652
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/master_bookmark   a075b5221b92
     default/withbook          0e7ec5675652

Try non fastfoward moves on regex bookmark
  $ hgedenapi push -r a075b5221b92 --to ffonly_bookmark --create -q
  $ hgedenapi push --non-forward-move --pushvar NON_FAST_FORWARD=true -r 0e7ec5675652 --to ffonly_bookmark
  pushing rev 0e7ec5675652 to destination https://localhost:$LOCAL_PORT/edenapi/ bookmark ffonly_bookmark
  moving remote bookmark ffonly_bookmark from a075b5221b92 to 0e7ec5675652
  abort: server error: invalid request: Non fast-forward bookmark move of 'ffonly_bookmark' from 29da74f8872f4ebf8d5221ad99c6684b24374922a8eb50b4b5bc4309602543b5 to 30c62517c166c69dc058930d510a6924d03d917d4e3a1354213faf4594d6e473
  [255]

Try to delete master
  $ cd ../repo-push
  $ hgedenapi push --delete master_bookmark
  deleting remote bookmark master_bookmark
  abort: failed to delete remote bookmark:
    remote server error: invalid request: Deletion of 'master_bookmark' is prohibited
  [255]

Delete the bookmark
  $ hgedenapi push --delete withbook
  deleting remote bookmark withbook
  $ cd ../repo-pull
  $ hgmn pull -q
  $ hg book --remote
     default/ffonly_bookmark   a075b5221b92
     default/master_bookmark   a075b5221b92
