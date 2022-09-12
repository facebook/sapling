#chg-compatible
  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord
  $ . "$TESTDIR/library.sh"

This file tests that normal mercurial operations never read the flat manifests


  $ cat >> $TESTTMP/flatcheck.py <<EOF
  > from __future__ import print_function
  > import sys, traceback
  > from edenscm import extensions, manifest
  > def uisetup(ui):
  >     extensions.wrapfunction(manifest.manifestrevlog, 'revision', readmf)
  > def readmf(orig, self, nodeorrev, **kwargs):
  >     if nodeorrev != -1:
  >         print('read flat manifest', file=sys.stderr)
  >         stack = traceback.extract_stack()
  >         print(''.join(traceback.format_list(stack[-3:-2])), file=sys.stderr)
  >     return orig(self, nodeorrev, **kwargs)
  > EOF

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/ext/treemanifestserver.py
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
  $ hg up 'desc(add)'
  fetching tree '' 5ce27016a79d253c34c64aebd35bfb09605ad3ee
  1 trees fetched over * (glob)
  8 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo a >> b && hg commit -Aqm 'modify b'
  $ hg rebase -d 77dc854aeab9a59885f87fa57bfeddbb73b23443 -r 'max(desc(modify))'
  rebasing 667a26a14261 "modify b"
  fetching tree '' 9486c937c5894f8f2adbaa0b589e8df5022217c9
  1 trees fetched over * (glob)
