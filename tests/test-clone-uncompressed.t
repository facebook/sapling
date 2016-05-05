#require serve

Initialize repository
the status call is to check for issue5130

  $ hg init server
  $ cd server
  $ touch foo
  $ hg -q commit -A -m initial
  >>> for i in range(1024):
  ...     with open(str(i), 'wb') as fh:
  ...         fh.write(str(i))
  $ hg -q commit -A -m 'add a lot of files'
  $ hg st
  $ hg serve -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS
  $ cd ..

Basic clone

  $ hg clone --uncompressed -U http://localhost:$HGPORT clone1
  streaming all changes
  1027 files to transfer, 96.3 KB of data
  transferred 96.3 KB in * seconds (*/sec) (glob)
  searching for changes
  no changes found

Clone with background file closing enabled

  $ hg --debug --config worker.backgroundclose=true --config worker.backgroundcloseminfilecount=1 clone --uncompressed -U http://localhost:$HGPORT clone-background | grep -v adding
  using http://localhost:$HGPORT/
  sending capabilities command
  sending branchmap command
  streaming all changes
  sending stream_out command
  1027 files to transfer, 96.3 KB of data
  starting 4 threads for background file closing
  transferred 96.3 KB in * seconds (*/sec) (glob)
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  no changes found
  sending getbundle command
  bundle2-input-bundle: with-transaction
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-part: total payload size 58
  bundle2-input-part: "listkeys" (params: 1 mandatory) supported
  bundle2-input-bundle: 1 parts total
  checking for updated bookmarks


Stream clone while repo is changing:

  $ mkdir changing
  $ cd changing

extension for delaying the server process so we reliably can modify the repo
while cloning

  $ cat > delayer.py <<EOF
  > import time
  > from mercurial import extensions, scmutil
  > def __call__(orig, self, path, *args, **kwargs):
  >     if path == 'data/f1.i':
  >         time.sleep(2)
  >     return orig(self, path, *args, **kwargs)
  > extensions.wrapfunction(scmutil.vfs, '__call__', __call__)
  > EOF

prepare repo with small and big file to cover both code paths in emitrevlogdata

  $ hg init repo
  $ touch repo/f1
  $ $TESTDIR/seq.py 50000 > repo/f2
  $ hg -R repo ci -Aqm "0"
  $ hg -R repo serve -p $HGPORT1 -d --pid-file=hg.pid --config extensions.delayer=delayer.py
  $ cat hg.pid >> $DAEMON_PIDS

clone while modifying the repo between stating file with write lock and
actually serving file content

  $ hg clone -q --uncompressed -U http://localhost:$HGPORT1 clone &
  $ sleep 1
  $ echo >> repo/f1
  $ echo >> repo/f2
  $ hg -R repo ci -m "1"
  $ wait
  $ hg -R clone id
  000000000000
