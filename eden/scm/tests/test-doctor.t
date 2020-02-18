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
  checking internal storage
  mutation: looks okay
  changelog: looks okay
  metalog: looks okay
  visibleheads: looks okay
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

Check the repo is broken (exit code is non-zero):

  $ hg log -GpT '{desc}\n' &>/dev/null
  [255]

Test that 'hg doctor' can fix them:

  $ hg doctor
  checking internal storage
  mutation: repaired
  changelog: looks okay
  metalog: repaired
  visibleheads: looks okay
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
  
#if py2
Check changelog repiar:

  $ newrepo
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  >>> with open(".hg/store/00changelog.i", "rb+") as f:
  ...     filelen = len(f.read())
  ...     f.seek(filelen - 64)
  ...     f.write(b"x" * 64)
  $ hg doctor
  checking internal storage
  mutation: looks okay
  changelog: corrupted at rev 2 (linkrev=2021161080)
  truncating 00changelog.i from 192 to 128 bytes
  truncating 00changelog.d from 165 to 110 bytes
  changelog: repaired
  metalog: looks okay
  visibleheads: removed 1 heads, added tip
  allheads: looks okay
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  |
  o  A
  

  $ hg status
#endif
