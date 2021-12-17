#chg-compatible

  $ configure modern
  $ setconfig format.use-symlink-atomic-write=1

Test indexedlogdatapack

  $ . "$TESTDIR/library.sh"

  $ newrepo master
  $ setconfig remotefilelog.server=true remotefilelog.serverexpiration=-1

  $ cd $TESTTMP
  $ enable remotenames
  $ setconfig remotefilelog.debug=false remotefilelog.write-hgcache-to-indexedlog=true remotefilelog.fetchpacks=true
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
  $ mv $TESTTMP/hgcache/master/manifests/indexedlogdatastore/latest{,.bak}
#if symlink
  $ ln -s foo $TESTTMP/hgcache/master/indexedlogdatastore/latest
  $ ln -s foo $TESTTMP/hgcache/master/manifests/indexedlogdatastore/latest
#else
  $ echo foo > $TESTTMP/hgcache/master/indexedlogdatastore/latest
  $ echo foo > $TESTTMP/hgcache/master/manifests/indexedlogdatastore/latest
#endif
  $ echo y > $TESTTMP/hgcache/master/indexedlogdatastore/0/index2-node
  $ echo y > $TESTTMP/hgcache/master/manifests/indexedlogdatastore/0/index2-node
  $ mkdir -p .hg/store/mutation/
  $ echo v > .hg/store/mutation/log
  $ echo xx > .hg/store/metalog/blobs/index2-id
  $ rm .hg/store/metalog/roots/meta
#if symlink
  $ ln -s foo .hg/store/metalog/roots/meta
#else
  $ echo foo > .hg/store/metalog/roots/meta
#endif
  $ rm .hg/store/allheads/meta

The repo is auto-fixed for common indexedlog open issues:
(note: this does not conver all corruption issues)

  $ hg log -GpT '{desc}\n'
  o  C
  │  diff --git a/C b/C
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/C
  │  @@ -0,0 +1,1 @@
  │  +C
  │  \ No newline at end of file
  │
  o  A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A
     \ No newline at end of file
  

Repairs log to "repair.log":

  $ cat .hg/store/mutation/repair.log
  date -d * (glob)
  Processing IndexedLog: * (glob)
  Verified 1 entries, 82 bytes in log
  Index "pred" passed integrity check
  Index "succ" passed integrity check
  Index "split" passed integrity check
  
  date -d * (glob)
  Corruption detected: * (glob)
  in * (glob)
  (This error is considered as a data corruption)
  Caused by 1 errors:
  - * (glob)
  Starting auto repair.
  
  date -d * (glob)
  Processing IndexedLog: * (glob)
  Fixed header in log
  Extended log to 82 bytes required by meta
  Verified first 0 entries, 12 of 82 bytes in log
  Backed up corrupted log to * (glob)
  Reset log size to 12
  Rebuilt index "pred"
  Rebuilt index "succ"
  Rebuilt index "split"
  

Test that 'hg doctor' can fix them:

  $ hg doctor -v
  checking internal storage
  metalog:
    Checking blobs at "*": (glob)
    Processing IndexedLog: * (glob)
    Verified * entries, * bytes in log (glob)
    Index "id" passed integrity check
    
    Checking roots at "*": (glob)
    Processing IndexedLog: * (glob)
    Verified 3 entries, 90 bytes in log
    Index "reverse" passed integrity check
    
    Checking blobs referred by 4 Roots:
    All Roots are verified.
  
  
  mutation:
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "pred" passed integrity check
    Index "succ" passed integrity check
    Index "split" passed integrity check
  
  
  allheads:
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
  
  
  revisionstore:
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Latest = 0
    Processing IndexedLog: * (glob)
    Verified 3 entries, 153 bytes in log
    Index "node" passed integrity check
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Index "sha256" passed integrity check
    Latest = 0
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "sha256" passed integrity check
    Latest = 0
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node_and_path" passed integrity check
    Latest = 0
    Processing IndexedLog: * (glob)
    Verified 3 entries, 357 bytes in log
    Index "node_and_path" passed integrity check
  
  
  revisionstore:
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Latest = 0
    Processing IndexedLog: * (glob)
    Verified 3 entries, 373 bytes in log
    Index "node" passed integrity check
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Index "sha256" passed integrity check
    Latest = 0
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "sha256" passed integrity check
    Latest = 0
    Processing RotateLog: "*" (glob)
    Attempt to repair log "0"
    Processing IndexedLog: * (glob)
    Verified 0 entries, 12 bytes in log
    Index "node_and_path" passed integrity check
    Latest = 0
    Processing IndexedLog: * (glob)
    Verified 3 entries, 348 bytes in log
    Index "node_and_path" passed integrity check
  
  
  checking commit references

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
  │
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
 (X might be unknown too)
  $ hg status -C
  M A2
  A A0
    A
  A X (?)
  R A
  R A1
  ? B
  ? C
  ? X (?)
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
  A X (?)
  R A
  R A1
  ? B
  ? C
  ? X (?)
  ? Y
  ? Z

Prepare new server repos

  $ newserver server
  $ clone server client1

  $ cd client1
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg push -r $A --to master --create -q

Test fixing master bookmark. Need the metalog (contains remotenames) to point
to commits unknown to the changelog. To do it, we "fork" the repo, and "pull"
on the forked repo, then replace the metalog from the old repo with the metalog
in the new repo, while keeping changelog unchanged.

  $ cd $TESTTMP
  $ clone server client2

  $ hg push --cwd client1 -r $B --to master -q

  $ cp -R client2 client3
  $ hg pull --cwd client3 -q

# Wipe it first, due to OSX disliking copying over symlinks
  $ rm -rf client2/.hg/store/metalog/*
  $ cp -R client3/.hg/store/metalog/* client2/.hg/store/metalog/

  $ cd client2
  $ hg doctor
  checking internal storage
  checking commit references
  remote/master points to an unknown commit - trying to move it to a known commit
  setting remote/master to 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  checking irrelevant draft branches for the workspace 'user/test/default'

  $ hg log -GT '{desc}\n'
  @  A
  
Test fixing broken segmented changelog (broken mutimeta)

  $ newrepo
  $ hg debugchangelog --migrate fullsegments
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ rm .hg/store/segments/v1/multimeta .hg/store/segments/v1/multimetalog/meta
  $ touch .hg/store/segments/v1/multimeta .hg/store/segments/v1/multimetalog/meta
  $ hg log -r tip 2>/dev/null 1>/dev/null

  $ hg doctor
  checking internal storage
  checking commit references

  $ hg log -r tip -T '{desc}\n'
  B

doctor should not remove draft for a segmented changelog repo

  $ newrepo
  $ hg debugchangelog --migrate fullsegments
  $ drawdag << 'EOS'
  > A B
  > EOS
  $ hg doctor
  checking internal storage
  checking commit references
  $ hg log -r 'all()' -T '{desc}'
  AB (no-eol)
