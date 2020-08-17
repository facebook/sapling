#chg-compatible

  $ enable gitlookup gitrevset extorder
  $ configure dummyssh
  $ . "$TESTDIR/infinitepush/library.sh"

Set up infinitepush and make sure it's loaded before gitrevset
  $ setupcommon
  $ setconfig extorder.infinitepush=gitlookup

  $ hg init repo
  $ cd repo
  $ setconfig gitlookup.mapfile="$TESTTMP/repo/.hg/git-mapfile"
  $ setupserver
  $ touch a
  $ hg add a
  $ hg ci -ma
  $ hg log -r . --template '{node}\n'
  3903775176ed42b1458a6281db4a0ccf4d9f287a
  $ cd .hg
  $ echo "ffffffffffffffffffffffffffffffffffffffff 3903775176ed42b1458a6281db4a0ccf4d9f287a" > git-mapfile

Clone a client and access git revision. Make sure it works
  $ cd ../..
  $ hg clone ssh://user@dummy/repo client
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd client
  $ hg log -r "gffffffffffffffffffffffffffffffffffffffff"
  commit:      3903775176ed
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
