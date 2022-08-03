#chg-compatible

#require execbit

  $ setconfig workingcopy.ruststatus=False
  $ configure modernclient

Create extension that can disable exec checks:

  $ cat > noexec.py <<EOF
  > from edenscm.mercurial import extensions, util
  > def setflags(orig, f, l, x):
  >     pass
  > def checkexec(orig, path):
  >     return False
  > def extsetup(ui):
  >     extensions.wrapfunction(util, 'setflags', setflags)
  >     extensions.wrapfunction(util, 'checkexec', checkexec)
  > EOF

  $ newclientrepo unix-repo test:server
  $ touch a
  $ hg add a
  $ hg commit -m 'unix: add a'
  $ hg push -q -r . --to book --create
  $ newclientrepo win-repo test:server book
  $ cd ../unix-repo
  $ chmod +x a
  $ hg commit -m 'unix: chmod a'
  $ hg push -q -r . --to book --create
  $ hg manifest -v
  755 * a

  $ cd ../win-repo

  $ touch b
  $ hg add b
  $ hg commit -m 'win: add b'

  $ hg manifest -v
  644   a
  644   b

  $ hg pull -B book
  pulling from test:server
  searching for changes

  $ hg manifest -v -r book
  755 * a

Simulate a Windows merge:

  $ hg --config extensions.n=$TESTTMP/noexec.py merge --debug
    searching for copies back to d6fa54f68ae1
    unmatched files in local:
     b
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: a03b0deabf2b, local: d6fa54f68ae1+, remote: 2d8bcf2dda39
   a: update permissions -> e
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Simulate a Windows commit:

  $ hg --config extensions.n=$TESTTMP/noexec.py commit -m 'win: merge'

  $ hg manifest -v
  755 * a
  644   b

  $ cd ..
