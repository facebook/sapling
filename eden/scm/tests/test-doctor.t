#chg-compatible

(This test needs to re-run the hg process. Therefore hard to use single-process Python test)

Test indexedlogdatapack

  $ . "$TESTDIR/library.sh"

  $ newrepo master
  $ setconfig remotefilelog.server=true remotefilelog.serverexpiration=-1

  $ cd $TESTTMP
  $ enable remotenames
  $ setconfig remotefilelog.debug=false remotefilelog.indexedlogdatastore=true remotefilelog.fetchpacks=true
  $ setconfig diff.git=true experimental.narrow-heads=true mutation.record=true mutation.enabled=true mutation.date="0 0" visibility.enabled=1

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  $ cd shallow

Make some commits

  $ drawdag << 'EOS'
  > B C  # amend: B -> C
  > |/
  > A
  > EOS

When everything looks okay:

  $ hg doctor
  mutation: looks okay
  metalog: looks okay
  allheads: looks okay
  indexedlogdatastore: looks okay

Break the repo in various ways:

  $ echo x > $TESTTMP/hgcache/master/indexedlogdatastore/latest
  $ echo y > $TESTTMP/hgcache/master/indexedlogdatastore/0/index-node.sum
  $ mkdir -p .hg/store/mutation/
  $ echo v > .hg/store/mutation/log
  $ echo xx > .hg/store/metalog/blobs/index-id
  $ echo xx > .hg/store/metalog/roots/meta
  $ rm .hg/store/allheads/meta

Check the repo is broken:

  $ hg log -GpT '{desc}\n'
  abort: "$TESTTMP/shallow/.hg/store/metalog/roots/meta": cannot read
  in log::OpenOptions::open("$TESTTMP/shallow/.hg/store/metalog/roots")
  Caused by 1 errors:
  - failed to fill whole buffer
  [255]

Test that 'hg doctor' can fix them:

  $ hg doctor
  mutation: repaired
  metalog: repaired
  allheads: repaired
  indexedlogdatastore: repaired

Check the repo is usable again:

  $ hg log -GpT '{desc}\n'
  o  C
  |  diff --git a/C b/C
  |  new file mode 100644
  |  --- /dev/null
  |  +++ b/C
  |  @@ -0,0 +1,1 @@
  |  +C
  |  \ No newline at end of file
  |
  o  A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A
     \ No newline at end of file
  
