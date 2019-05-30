  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > infinitepushbackup=
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -m 'add a'
  adding a
  $ hg log -T '{short(node)}\n'
  ac82d8b1f7c4

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push, repo-2 and repo-3
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-2 --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-3 --noupdate

blobimport

  $ blobimport repo-hg/.hg repo
  $ sqlite3 "$TESTTMP/repo/bookmarks" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo

create new bookmarks, then update their properties
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch b && hg addremove && hg ci -q -m 'add b'
  adding b
  $ hgmn push ssh://user@dummy/repo -r . --to "not_pull_default" --create
  pushing rev 907767d421e4 to destination ssh://user@dummy/repo bookmark not_pull_default
  searching for changes
  exporting bookmark not_pull_default
  $ touch c && hg addremove && hg ci -q -m 'add c'
  adding c
  $ hgmn push ssh://user@dummy/repo -r . --to "scratch" --create
  pushing rev b2d646f64a99 to destination ssh://user@dummy/repo bookmark scratch
  searching for changes
  exporting bookmark scratch
  $ sqlite3 "$TESTTMP/repo/bookmarks" "UPDATE bookmarks SET hg_kind = CAST('scratch' AS BLOB) WHERE name LIKE 'scratch';"
  $ sqlite3 "$TESTTMP/repo/bookmarks" "UPDATE bookmarks SET hg_kind = CAST('publishing' AS BLOB) WHERE name LIKE 'not_pull_default';"
  $ sqlite3 "$TESTTMP/repo/bookmarks" 'SELECT name, hg_kind FROM bookmarks;'
  master_bookmark|pull_default
  not_pull_default|publishing
  scratch|scratch
  $ tglogpnr
  @  b2d646f64a99 public 'add c'
  |
  o  907767d421e4 public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
test publishing
  $ cd "$TESTTMP/repo-2"
  $ tglogpnr
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
  $ hgmn pull
  pulling from ssh://user@dummy/rep* (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 907767d421e4
  $ hgmn up 907767d421e4cb28c7978bedef8ccac7242b155e
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hgmn up b2d646f64a9978717516887968786c6b7a33edf9
  'b2d646f64a9978717516887968786c6b7a33edf9' does not exist locally - looking for it remotely...
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets b2d646f64a99
  'b2d646f64a9978717516887968786c6b7a33edf9' found remotely
  pull finished in * (glob)
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ tglogpnr
  @  b2d646f64a99 draft 'add c'
  |
  o  907767d421e4 public 'add b'
  |
  o  ac82d8b1f7c4 public 'add a' master_bookmark
  
  $ hgmn bookmarks
     master_bookmark           0:ac82d8b1f7c4
  $ hgmn bookmarks --list-remote "*"
     master_bookmark           ac82d8b1f7c418c61a493ed229ffaa981bda8e90
     not_pull_default          907767d421e4cb28c7978bedef8ccac7242b155e
     scratch                   b2d646f64a9978717516887968786c6b7a33edf9
