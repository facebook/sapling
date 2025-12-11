# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"
  $ setconfig ui.ignorerevnum=false

setup configuration
  $ setup_common_config "blob_sqlite"
  $ REPOID=1 REPONAME=disabled_repo ENABLED=false setup_mononoke_config
  $ cd $TESTTMP

  $ setconfig remotenames.selectivepulldefault=master_bookmark,master_bookmark2
  $ setconfig experimental.new-clone-path=true

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


start mononoke

  $ start_and_wait_for_mononoke_server

setup repo

  $ hg clone -q mono:repo repo --noupdate
  $ cd repo

Create linear chain commits
  $ touch a
  $ hg add a
  $ hg ci -qAm a
  $ A_NODE=$(hg log -r . -T '{node}')

  $ touch b
  $ hg add b
  $ hg ci -qAm b

  $ echo content > c
  $ hg add c
  $ hg ci -qAm c

  $ mkdir dir
  $ echo 1 > dir/1
  $ mkdir dir2
  $ echo 2 > dir/2
  $ hg addremove -q
  $ hg ci -qAm 'new directory'

  $ echo cc > c
  $ hg ci -qAm 'modify file'

  $ hg mv dir/1 dir/rename
  $ hg ci -qAm 'rename'
  $ RENAME_NODE=$(hg log -r . -T '{node}')

Create disconnected merge DAG
  $ hg update -q null
  $ echo "x" > D
  $ hg commit -qAm A
  $ A_DRAWDAG_NODE=$(hg log -r . -T '{node}')

  $ echo "1" > D
  $ hg commit -qAm B
  $ B_NODE=$(hg log -r . -T '{node}')

  $ hg update -q $A_DRAWDAG_NODE
  $ echo "2" > D
  $ hg commit -qAm C
  $ C_NODE=$(hg log -r . -T '{node}')

  $ hg update -q $B_NODE
  $ hg merge -q $C_NODE || true
  warning: 1 conflicts while merging D! (edit, then use 'hg resolve --mark')
  $ printf '1\n2\n' > D
  $ hg resolve -m D
  (no more unresolved files)
  $ hg commit -qAm D
  $ D_NODE=$(hg log -r . -T '{node}')

Push both histories
  $ hg push --to master_bookmark --create -r $D_NODE -q
  $ hg push --to master_bookmark2 --create -r $RENAME_NODE -q

Export variables for use in other repos
  $ export A_NODE
  $ export RENAME_NODE
  $ export C_NODE
  $ export D_NODE

  $ hg log --graph -T '{node|short} {desc}'
  o  9f8e7242d9fa rename
  │
  o  586ef37a04f7 modify file
  │
  o  e343d2f326cf new directory
  │
  o  3e19bf519e9a c
  │
  o  0e067c57feba b
  │
  o  3903775176ed a
  
  @    1aceff28fe8c D
  ├─╮
  │ o  8a2febcd784f B
  │ │
  o │  d64c36ecf9aa C
  ├─╯
  o  295f4466d891 A
  


  $ cd $TESTTMP

  $ hg debugwireargs mono:disabled_repo one two --three three
  remote: Unknown Repo:
  remote:   Error:
  remote:     Requested repo "disabled_repo" does not exist or is disabled
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hg debugwireargs mono:repo one two --three three
  one two three None None

setup repo2
  $ hg clone -q mono:repo repo2 --noupdate
  $ cd repo2
  $ hg pull -q
  $ hg up -q $A_NODE
Test a pull of one specific revision
  $ hg pull -r $C_NODE -q
(with selectivepull, pulling a commit hash also pulls the selected bookmarks)

  $ hg log -r "$A_NODE::$RENAME_NODE" --graph  -T '{node|short} {desc}'
  o  9f8e7242d9fa rename
  │
  o  586ef37a04f7 modify file
  │
  o  e343d2f326cf new directory
  │
  o  3e19bf519e9a c
  │
  o  0e067c57feba b
  │
  @  3903775176ed a
   (re)

  $ ls
  a
  $ hg up $RENAME_NODE -q
  $ ls
  a
  b
  c
  dir
  $ cat c
  cc
  $ hg up $RENAME_NODE -q
  $ hg log c -T '{node|short} {desc}\n'
  warning: file log can be slow on large repos - use -f to speed it up
  586ef37a04f7 modify file
  3e19bf519e9a c
  $ cat dir/rename
  1
  $ cat dir/2
  2
  $ hg log dir/rename -f -T '{node|short} {desc}\n'
  9f8e7242d9fa rename
  e343d2f326cf new directory
  $ hg st --change $RENAME_NODE -C
  A dir/rename
    dir/1
  R dir/1

  $ hg up -q $D_NODE

Sort the output because it may be unpredictable because of the merge
  $ hg log D --follow -T '{node|short} {desc}\n' | sort
  1aceff28fe8c D
  295f4466d891 A
  8a2febcd784f B
  d64c36ecf9aa C


Create a new bookmark and try and send it over the wire
Test commented while we have no bookmark support in blobimport or easy method
to create a fileblob bookmark
#  $ cd ../repo
#  $ hg bookmark test-bookmark
#  $ hg bookmarks
#   * test-bookmark             0:3903775176ed
#  $ cd ../repo2
#  $ hg pull ssh://user@dummy/repo
#  pulling from ssh://user@dummy/repo
#  searching for changes
#  no changes found
#  adding remote bookmark test-bookmark
#  $ hg bookmarks
#     test-bookmark             0:3903775176ed

Do a clone of the repo
  $ hg clone -U mono:repo repo-streamclone
  fetching lazy changelog
  populating main commit graph
