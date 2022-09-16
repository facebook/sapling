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

  $ configure selectivepull
  $ setconfig remotenames.selectivepulldefault=master_bookmark,master_bookmark2
  $ setconfig experimental.new-clone-path=true

setup common configuration
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > ssh="$DUMMYSSH"
  > EOF


setup repo

  $ hg init repo-hg

Init treemanifest and remotefilelog
  $ cd repo-hg
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=!
  > treemanifestserver=
  > remotefilelog=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log
  commit:      3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
  $ hg bookmark -r . master_bookmark
  $ cd $TESTTMP

setup repo2
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > remotefilelog=
  > [remotefilelog]
  > cachepath=$TESTTMP/cachepath
  > EOF
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo2 --noupdate
  $ cd repo2
  $ hg pull
  pulling from ssh://user@dummy/repo-hg

  $ cd $TESTTMP
  $ cd repo-hg
  $ touch b
  $ hg add b
  $ hg ci -mb
  $ echo content > c
  $ hg add c
  $ hg ci -mc
  $ mkdir dir
  $ echo 1 > dir/1
  $ mkdir dir2
  $ echo 2 > dir/2
  $ hg addremove
  adding dir/1
  adding dir/2
  $ hg ci -m 'new directory'
  $ echo cc > c
  $ hg addremove
  $ hg ci -m 'modify file'
  $ hg mv dir/1 dir/rename
  $ hg ci -m 'rename'
  $ hg debugdrawdag <<'EOS'
  >   D  # D/D=1\n2\n
  >  /|  # B/D=1\n
  > B C  # C/D=2\n
  > |/   # A/D=x\n
  > A
  > EOS
  $ hg log --graph -T '{node|short} {desc}'
  o    e635b24c95f7 D
  ├─╮
  │ o  d351044ef463 C
  │ │
  o │  9a827afb7e25 B
  ├─╯
  o  af6aa0dfdf3d A
   (re)
  @  9f8e7242d9fa rename
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
   (re)

setup master bookmarks

  $ hg bookmark master_bookmark -fr e635b24c95f7
  $ hg bookmark master_bookmark2 -r 9f8e7242d9fa

blobimport

  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke

  $ start_and_wait_for_mononoke_server
  $ hgmn debugwireargs mononoke://$(mononoke_address)/disabled_repo one two --three three
  remote: Requested repo "disabled_repo" does not exist or is disabled
  abort: unexpected EOL, expected netstring digit
  [255]
  $ hgmn debugwireargs mononoke://$(mononoke_address)/repo one two --three three
  one two three None None

  $ cd repo2
  $ hg up -q 0
Test a pull of one specific revision
  $ hgmn pull -r 3e19bf519e9af6c66edf28380101a92122cbea50 -q
(with selectivepull, pulling a commit hash also pulls the selected bookmarks)

  $ hg log -r '3903775176ed::586ef37a04f7' --graph  -T '{node|short} {desc}'
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
  $ hgmn up 9f8e7242d9fa -q
  $ ls
  a
  b
  c
  dir
  $ cat c
  cc
  $ hgmn up 9f8e7242d9fa -q
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
  $ hg st --change 9f8e7242d9fa -C
  A dir/rename
    dir/1
  R dir/1

  $ hgmn up -q e635b24c95f7

Sort the output because it may be unpredictable because of the merge
  $ hg log D --follow -T '{node|short} {desc}\n' | sort
  9a827afb7e25 B
  af6aa0dfdf3d A
  d351044ef463 C
  e635b24c95f7 D

Create a new bookmark and try and send it over the wire
Test commented while we have no bookmark support in blobimport or easy method
to create a fileblob bookmark
#  $ cd ../repo
#  $ hg bookmark test-bookmark
#  $ hg bookmarks
#   * test-bookmark             0:3903775176ed
#  $ cd ../repo2
#  $ hgmn pull ssh://user@dummy/repo
#  pulling from ssh://user@dummy/repo
#  searching for changes
#  no changes found
#  adding remote bookmark test-bookmark
#  $ hg bookmarks
#     test-bookmark             0:3903775176ed

Do a streaming clone of the repo
  $ hgmn clone -U --stream mononoke://$(mononoke_address)/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true
  fetching changelog
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (* bytes/sec) (glob)
  fetching selected remote bookmarks
