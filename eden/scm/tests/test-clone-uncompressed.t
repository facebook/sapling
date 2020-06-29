#chg-compatible

  $ disable treemanifest
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
  1028 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found

Block full streaming clones
  $ cat >> server/.hg/hgrc <<EOF
  > [server]
  > requireexplicitfullclone=True
  > EOF
  $ hg clone --stream -U ssh://user@dummy/server blockedclone
  streaming all changes
  remote: unable to perform an implicit streaming clone - make sure remotefilelog is enabled (?)
  abort: locking the remote repository failed
  remote: unable to perform an implicit streaming clone - make sure remotefilelog is enabled (?)
  [255]
  $ hg clone --stream --config clone.requestfullclone=True -U ssh://user@dummy/server blockedclone
  streaming all changes
  * files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found
  $ rm server/.hg/hgrc

--uncompressed is an alias to --stream

  $ hg clone --uncompressed -U ssh://user@dummy/server clone1-uncompressed
  streaming all changes
  1028 files to transfer, * of data (glob)
  transferred * in * seconds (*) (glob)
  searching for changes
  no changes found

Clone with background file closing enabled

  $ hg --debug --config worker.backgroundclose=true --config worker.backgroundcloseminfilecount=1 clone --stream -U ssh://user@dummy/server clone-background | grep -v adding
  running * 'user@dummy' 'hg -R server serve --stdio' (glob)
  sending hello command
  sending between command
  remote: 398
  remote: capabilities: lookup changegroupsubset branchmap pushkey known getbundle unbundlehash unbundlereplay batch streamreqs=generaldelta,revlogv1 stream_option $USUAL_BUNDLE2_CAPS$ unbundle=HG10GZ,HG10BZ,HG10UN
  remote: 1
  streaming all changes
  sending stream_out_option command
  1028 files to transfer, * of data (glob)
  starting 4 threads for background file closing
  transferred * in * seconds (*) (glob)
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  no changes found
  sending getbundle command
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-part: "phase-heads" supported
  bundle2-input-part: total payload size 24
  bundle2-input-bundle: 1 parts total
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
  $ $TESTDIR/seq.py 50000 > repo/f2
  $ hg -R repo ci -Aqm "0"

  $ cat >>repo/.hg/hgrc <<EOF
  > [extensions]
  > delayer=$TESTTMP/changing/delayer.py
  > EOF

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
