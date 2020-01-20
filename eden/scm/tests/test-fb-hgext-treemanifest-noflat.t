#chg-compatible

TODO: configure mutation
  $ configure noevolution
  $ . "$TESTDIR/library.sh"

This file tests that normal mercurial operations never read the flat manifests


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

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF
  $ cd ..
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
  > flatcheck=$TESTTMP/flatcheck.py
  > 
  > [remotefilelog]
  > reponame=master
  > 
  > [treemanifest]
  > autocreatetrees=True
  > EOF

  $ hg pull -q -r 0
  $ hg pull -q -r 1
  $ hg up 0
  fetching tree '' 5ce27016a79d253c34c64aebd35bfb09605ad3ee
  1 trees fetched over * (glob)
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo a >> b && hg commit -Aqm 'modify b'
  $ hg rebase -d 1 -r 2
  rebasing 667a26a14261 "modify b"
  fetching tree '' 9486c937c5894f8f2adbaa0b589e8df5022217c9, based on 5ce27016a79d253c34c64aebd35bfb09605ad3ee, found via 77dc854aeab9
  1 trees fetched over * (glob)
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/667a26a14261-d769c687-rebase.hg (glob)
