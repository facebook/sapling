  $ setconfig extensions.treemanifest=!
# Integration tests between tree and fastmanifest

  $ setconfig treemanifest.treeonly=False
  $ . "$TESTDIR/library.sh"


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
  $ cd master
  $ echo a > a && hg ci -Aqm 'added a'
  $ cd ..

  $ hg clone -q ssh://user@dummy/master client
  $ cd master
  $ echo b > b && hg ci -Aqm 'added b'
  $ echo c > c && hg ci -Aqm 'added c'
  $ cd ..

  $ cd client
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
  > usecache=True
  > 
  > [treemanifest]
  > autocreatetrees=True
  > demanddownload=False
  > EOF
  $ hg pull -q
  read flat manifest
    File "*fastmanifest/implementation.py", line *, in loadflat (glob)
      data = self.revlog.revision(self._node)
  

# Test checking out from a fastmanifest to a treemanifest uses the treemanifest
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d > d && hg ci -Aqm 'added d'
  read flat manifest
    File "*fastmanifest/implementation.py", line *, in add (glob)
      p1text = origself.revision(p1)
  
  $ hg debugcachemanifest -r .
  read flat manifest
    File "*fastmanifest/implementation.py", line *, in loadflat (glob)
      data = self.revlog.revision(self._node)
  
  $ hg diff -r tip -r 1 --stat
   c |  1 -
   d |  1 -
   2 files changed, 0 insertions(+), 2 deletions(-)
  $ hg diff -r 1 -r tip --stat
   c |  1 +
   d |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)
