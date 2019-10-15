  $ setconfig extensions.treemanifest=!
TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ setconfig treemanifest.treeonly=False
  $ . "$TESTDIR/library.sh"

This file tests that normal mercurial operations almost never read the flat manifests


  $ cat >> $TESTTMP/flatcheck.py <<EOF
  > import sys, traceback
  > from edenscm.mercurial import extensions, manifest
  > def uisetup(ui):
  >     extensions.wrapfunction(manifest.manifestrevlog, 'revision', readmf)
  > def readmf(orig, self, nodeorrev, **kwargs):
  >     if nodeorrev != -1:
  >         print >> sys.stderr, 'read flat manifest'
  >         stack = traceback.extract_stack()
  >         print >> sys.stderr, ''.join(traceback.format_list(stack[-3:-2]))
  >     return orig(self, nodeorrev, **kwargs)
  > EOF

  $ hg init master
  $ hg clone -q ssh://user@dummy/master client

- Add a bunch of files so the manifest is large enough to use deltas
  $ cd master
  $ echo a >> a
  $ echo a >> b
  $ echo a >> c
  $ echo a >> d
  $ echo a >> e
  $ echo a >> f
  $ echo a >> g
  $ echo a >> h
  $ hg commit -Aqm 'add a-f'
  $ echo a >> a
  $ hg commit -Aqm 'modify a'

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > fastmanifest=
  > flatcheck=$TESTTMP/flatcheck.py
  > treemanifest=
  > 
  > [remotefilelog]
  > reponame=master
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > 
  > [treemanifest]
  > autocreatetrees=True
  > EOF

# Test there is one flat read is expected on the first pull, since
# manifest.revdiff cannot hit the fast path since the first manifest is not a delta.
  $ hg pull -q -r 0
  read flat manifest
    File "*/mercurial/revlog.py", line *, in revdiff (glob)
      self.revision(rev* (glob)
  
# Test that no flat manifests are read during pull and update
  $ hg pull -q -r 1
  $ hg up 0
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved

# Test only one flat read is expected on each commit, to get the p1 fulltext to
# produce the delta.
  $ echo a >> b && hg commit -Aqm 'modify b'
  read flat manifest
    File "*/fastmanifest/implementation.py", line *, in add (glob)
      p1text = origself.revision(p1)
  
# Test that rebase access the flat text only once, for the final commit
  $ hg rebase -d 1 -r 2
  read flat manifest
    File "*/fastmanifest/implementation.py", line *, in add (glob)
      p1text = origself.revision(p1)
  
  rebasing 667a26a14261 "modify b" (tip)
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/667a26a14261-d769c687-rebase.hg (glob)
