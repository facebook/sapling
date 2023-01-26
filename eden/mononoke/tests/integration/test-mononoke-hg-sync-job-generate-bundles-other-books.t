# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration

  $ setup_common_config blob_files
  $ cd $TESTTMP

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ echo foo > a
  $ echo foo > b
  $ hg addremove && hg ci -m 'initial'
  adding a
  adding b
  $ echo 'bar' > a
  $ hg addremove && hg ci -m 'a => bar'
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > EOF

create master bookmark

  $ hg bookmark master_bookmark -r tip

blobimport them into Mononoke storage and start Mononoke
  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke
  $ start_and_wait_for_mononoke_server
Make client repo
  $ hgclone_treemanifest ssh://user@dummy/repo-hg client-push --noupdate --config extensions.remotenames= -q

Push to Mononoke
  $ cd $TESTTMP/client-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase =
  > remotenames =
  > EOF
  $ hg up -q tip

  $ mkcommit pushcommit
  $ hgmn push -r . --to newbook --create -q
  $ BOOK_LOC=$(hg log -r newbook -T '{node}')

Force push
  $ hg up -q "min(all())"
  $ mkcommit forcepush
  $ hgmn push -r . --to newbook --create -q

Bookmark move
  $ hgmn push -r "$BOOK_LOC" --to newbook --pushvar NON_FAST_FORWARD=true
  pushing rev 1e43292ffbb3 to destination mononoke://$LOCALIP:$LOCAL_PORT/repo bookmark newbook
  searching for changes
  no changes found
  updating bookmark newbook

Force push of unrelated commit stack containing empty tree
  $ hg update -q null
  $ mkcommit unrelated
  $ hg rm unrelated
  $ hg commit --amend
  $ mkcommit unrelated2
  $ mkcommit unrelated3
  $ hgmn push -r . --to newbook --non-forward-move --create --force -q
move back
  $ mononoke_newadmin bookmarks -R repo set newbook "$BOOK_LOC"
  Updating publishing bookmark newbook from 37ea8302245e8e828fa0745759441d4e9cd99cb0e4ffb045096c5fadc3207274 to 9243ee8d4ea76ca29fb3135f85b9596eb51688fd06347983c449ed1eec255345

Delete a bookmark
  $ hgmn push --delete newbook
  pushing to mononoke://$LOCALIP:$LOCAL_PORT/repo
  searching for changes
  no changes found
  deleting remote bookmark newbook
  [1]

Verify that the entries are in update log
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" "select id, hex(from_changeset_id), hex(to_changeset_id), reason from bookmarks_update_log;"
  1||59443DD90EF0D496A30B73639C6601566ACF9680E320895903AAF7F3380C07F2|blobimport
  2||9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345|pushrebase
  3|9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345|E7526D79596291BFAB4A40E177157BAE556E11F5F8F137FB751140EF3C65DEA2|pushrebase
  4|E7526D79596291BFAB4A40E177157BAE556E11F5F8F137FB751140EF3C65DEA2|9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345|pushrebase
  5|9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345|37EA8302245E8E828FA0745759441D4E9CD99CB0E4FFB045096C5FADC3207274|pushrebase
  6|37EA8302245E8E828FA0745759441D4E9CD99CB0E4FFB045096C5FADC3207274|9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345|manualmove
  7|9243EE8D4EA76CA29FB3135F85B9596EB51688FD06347983C449ED1EEC255345||pushrebase

Sync it to another client
  $ cd $TESTTMP/repo-hg
  $ enable_replay_verification_hook
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
  $ cd $TESTTMP

Sync a creation of a bookmark
  $ mononoke_hg_sync repo-hg 1 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [2]* (glob)

  $ cd $TESTTMP/repo-hg
  $ hg log -r newbook -T '{desc}'
  pushcommit (no-eol)
  $ cd -
  $TESTTMP

Sync force push
  $ mononoke_hg_sync repo-hg 2 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [3]* (glob)

Sync bookmark move
  $ mononoke_hg_sync repo-hg 3 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [4]* (glob)

  $ cd $TESTTMP/repo-hg && hg log -r newbook -T "{desc}\n" && cd -
  pushcommit
  $TESTTMP

Sync force push of unrelated commit stack containing empty tree
  $ mononoke_hg_sync repo-hg 4 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [5]* (glob)

  $ cd $TESTTMP/repo-hg && hg log -r newbook -T "{desc}\n" && cd -
  unrelated3
  $TESTTMP

..and move the bookmark back (via mononoke-admin)
  $ mononoke_hg_sync repo-hg 5 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [6]* (glob)

Sync deletion of a bookmark
  $ mononoke_hg_sync repo-hg 6 2>&1 | grep 'successful sync of entries'
  * successful sync of entries [7]* (glob)

  $ cd $TESTTMP/repo-hg
  $ hg log -r newbook
  abort: unknown revision 'newbook'!
  [255]
