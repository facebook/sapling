  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ setup_mononoke_config
  $ REPOID=1 REPONAME=disabled_repo ENABLED=false setup_mononoke_config
  $ cd $TESTTMP

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
  > treemanifest=
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
  changeset:   0:3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
   (re)
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
  searching for changes
  no changes found

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
  |\
  | o  d351044ef463 C
  | |
  o |  9a827afb7e25 B
  |/
  o  af6aa0dfdf3d A
   (re)
  @  28468743616e rename
  |
  o  329b10223740 modify file
  |
  o  a42a44555d7c new directory
  |
  o  3e19bf519e9a c
  |
  o  0e067c57feba b
  |
  o  3903775176ed a
   (re)

setup master bookmarks

  $ hg bookmark master_bookmark -r e635b24c95f7
  $ hg bookmark master_bookmark2 -r 28468743616e

blobimport

  $ cd ..
  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke
  $ hgmn debugwireargs ssh://user@dummy/disabled_repo one two --three three
  remote: * ERRO Requested repo "disabled_repo" does not exist or disabled (glob)
  abort: no suitable response from remote hg!
  [255]
  $ hgmn debugwireargs ssh://user@dummy/repo one two --three three
  one two three None None

  $ cd repo2
  $ hg up -q 0
Test a pull of one specific revision
  $ hgmn pull -r 3e19bf519e9af6c66edf28380101a92122cbea50 -q
Pull the rest
  $ hgmn pull -q

  $ hg log -r '3903775176ed::329b10223740' --graph  -T '{node|short} {desc}'
  o  329b10223740 modify file
  |
  o  a42a44555d7c new directory
  |
  o  3e19bf519e9a c
  |
  o  0e067c57feba b
  |
  @  3903775176ed a
   (re)
  $ ls
  a
  $ hgmn up 28468743616e -q
  $ ls
  a
  b
  c
  dir
  $ cat c
  cc
  $ hgmn up 28468743616e -q
  $ hg log c -T '{node|short} {desc}\n'
  warning: file log can be slow on large repos - use -f to speed it up
  329b10223740 modify file
  3e19bf519e9a c
  $ cat dir/rename
  1
  $ cat dir/2
  2
  $ hg log dir/rename -f -T '{node|short} {desc}\n'
  28468743616e rename
  a42a44555d7c new directory
  $ hg st --change 28468743616e -C
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
  $ hgmn clone --stream ssh://user@dummy/repo repo-streamclone --config extensions.treemanifest= --config remotefilelog.reponame=master --shallow --config treemanifest.treeonly=true --config extensions.lz4revlog=
  streaming all changes
  2 files to transfer, * bytes of data (glob)
  transferred * bytes in * seconds (* bytes/sec) (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 10 changesets with 0 changes to 0 files
  updating to branch default
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
