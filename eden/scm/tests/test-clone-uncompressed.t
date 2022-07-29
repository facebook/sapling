#chg-compatible

  $ configure dummyssh
#require serve

Initialize repository
the status call is to check for issue5130

  $ hg init server
  $ cd server
  $ touch foo
  $ hg -q commit -A -m initial
  >>> for i in range(1024):
  ...     with open(str(i), 'w') as fh:
  ...         x = fh.write("%s" % (str(i),))
  $ hg -q commit -A -m 'add a lot of files'
  $ hg st
  $ cd ..

Basic clone

  $ hg clone --stream -U ssh://user@dummy/server clone1
  streaming all changes
  1025 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes

Clone with background file closing enabled

  $ hg --debug --config worker.backgroundclose=true --config worker.backgroundcloseminfilecount=1 clone --stream -U ssh://user@dummy/server clone-background 2>&1 | grep -v adding
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 448
  remote: capabilities: lookup changegroupsubset branchmap pushkey known getbundle unbundlehash unbundlereplay batch streamreqs=generaldelta,lz4revlog,revlogv1 stream_option $USUAL_BUNDLE2_CAPS$%0Atreemanifest%3DTrue%0Atreeonly%3DTrue unbundle=HG10GZ,HG10BZ,HG10UN
  remote: 1
  streaming all changes
  sending stream_out_option command
  1025 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  query 1; heads
  sending batch command
  requesting all changes
  sending getbundle command
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "changegroup" (params: 1 mandatory 1 advisory) supported
  bundle2-input-part: total payload size 137448
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size 46374
  bundle2-input-bundle: 2 parts total
  checking for updated bookmarks

Stream clone while repo is changing:

  $ mkdir changing
  $ cd changing

extension for delaying the server process so we reliably can modify the repo
while cloning

  $ cat > delayer.py <<EOF
  > import time
  > from edenscm.mercurial import extensions, vfs
  > def __call__(orig, self, path, *args, **kwargs):
  >     if path == 'data/f1.i':
  >         time.sleep(2)
  >     return orig(self, path, *args, **kwargs)
  > extensions.wrapfunction(vfs.vfs, '__call__', __call__)
  > EOF

prepare repo with small and big file to cover both code paths in emitrevlogdata

  $ hg init repo
  $ touch repo/f1
  $ seq 50000 > repo/f2
  $ hg -R repo ci -Aqm "0"

  $ cat >>repo/.hg/hgrc <<EOF
  > [extensions]
  > delayer=$TESTTMP/changing/delayer.py
  > EOF

#if bash
clone while modifying the repo between stating file with write lock and
actually serving file content

  $ hg clone -q --stream -U ssh://user@dummy/changing/repo clone &
  $ sleep 1
  $ echo >> repo/f1
  $ echo >> repo/f2
  $ hg -R repo ci -m "1"
  $ wait
  $ hg -R clone id
  000000000000
#endif
