#chg-compatible

(This test needs to re-run the hg process. Therefore hard to use single-process Python test)

  $ setconfig format.use-symlink-atomic-write=1

Enable writing to hgcommits/v1:

  $ setconfig format.use-zstore-commit-data=1 format.use-zstore-commit-data-revlog-fallback=1

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
  checking commit references

Break the repo in various ways:

  $ mv $TESTTMP/hgcache/master/indexedlogdatastore/latest{,.bak}
#if symlink
  $ ln -s foo $TESTTMP/hgcache/master/indexedlogdatastore/latest
#else
  $ echo foo > $TESTTMP/hgcache/master/indexedlogdatastore/latest
#endif
  $ echo y > $TESTTMP/hgcache/master/indexedlogdatastore/0/index2-node
  $ mkdir -p .hg/store/mutation/
  $ echo v > .hg/store/mutation/log
  $ echo xx > .hg/store/metalog/blobs/index2-id
  $ rm .hg/store/metalog/roots/meta
#if symlink
  $ ln -s foo .hg/store/metalog/roots/meta
#else
  $ echo foo > .hg/store/metalog/roots/meta
#endif
  $ rm .hg/store/hgcommits/v1/index2-id
  $ rm .hg/store/allheads/meta

Check the repo is broken (exit code is non-zero):

  $ hg log -GpT '{desc}\n' &>/dev/null
  [255]

Test that 'hg doctor' can fix them:

  $ hg doctor
  checking internal storage
  mutation: repaired
  hgcommits/v1: repaired
  metalog: repaired
  allheads: repaired
  indexedlogdatastore: repaired
  checking commit references

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
  ...     x = f.seek(filelen - 64)
  ...     x = f.write(b"x" * 64)
  $ hg doctor
  checking internal storage
  changelog: corrupted at rev 2 (linkrev=2021161080)
  truncating 00changelog.i from 192 to 128 bytes
  truncating 00changelog.d from 165 to 110 bytes
  changelog: repaired
  visibleheads: removed 1 heads, added tip
  checking commit references
  $ hg log -Gr 'all()' -T '{desc}'
  o  B
  |
  o  A
  

  $ hg status

Check unknown visibleheads format:

  $ newrepo
  $ hg dbsh << 'EOS'
  > ml = repo.svfs.metalog
  > ml.set("visibleheads", b"v-1")
  > ml.commit("break visibleheads")
  > EOS
  $ hg doctor
  checking internal storage
  visibleheads: removed 0 heads, added tip
  checking commit references

Check dirstate pointing to a stripped commit:

  $ newrepo
  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |   # A/A2=2
  > A   # A/A1=1
  > EOS

  $ hg up -q 'desc(A)'
  $ hg st
  $ hg mv A A0
  $ hg rm A1
  $ echo 3 > A2
  $ echo 1 > X
  $ hg add X
  $ hg up -q 'desc(B)'
  $ echo 4 > B
  $ echo 2 > Y
  $ hg add Y
  $ hg up -q 'desc(C)'
  $ echo 3 > Z
  $ hg add Z

  $ hg status -C
  M A2
  M B
  A A0
    A
  A X
  A Y
  A Z
  R A
  R A1

 (strip 2 commits while preserving the treestate)
  >>> with open(".hg/store/00changelog.i", "rb+") as f:
  ...     x = f.truncate(64)  # only keep 1 commit: "A"

XXX: The error message should be improved to indicate it is revlog and hg doctor might fix it.

  $ hg status
  abort: mmap length 192 is greater than file size 64
  [255]

 (hg doctor can fix dirstate/treestate)
  $ hg doctor
  checking internal storage
  visibleheads: removed 1 heads, added tip
  treestate: repaired
  checking commit references

  $ hg log -r . -T '{desc}\n'
  A

 (dirstate reverted to a previous state: B, C, X, Y, Z become unknown)
  $ hg status -C
  M A2
  A A0
    A
  A X
  R A
  R A1
  ? B
  ? C
  ? Y
  ? Z

Try other kinds of dirstate corruptions:

  >>> with open(".hg/dirstate", "rb+") as f:
  ...     x = f.seek(0)
  ...     x = f.write(b"x" * 1024)
  $ hg doctor
  checking internal storage
  treestate: repaired
  checking commit references
  $ hg status
  M A2
  A A0
  A X
  R A
  R A1
  ? B
  ? C
  ? Y
  ? Z
