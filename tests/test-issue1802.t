Create extension that can disable exec checks:

  $ cat > noexec.py <<EOF
  > from mercurial import extensions, util
  > def setflags(orig, f, l, x):
  >     pass
  > def checkexec(orig, path):
  >     return False
  > def extsetup(ui):
  >     extensions.wrapfunction(util, 'setflags', setflags)
  >     extensions.wrapfunction(util, 'checkexec', checkexec)
  > EOF

  $ hg init unix-repo
  $ cd unix-repo
  $ touch a
  $ hg add a
  $ hg commit -m 'unix: add a'
  $ hg clone . ../win-repo
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ chmod +x a
  $ hg commit -m 'unix: chmod a'
  $ hg manifest -v
  755 * a

  $ cd ../win-repo

  $ touch b
  $ hg add b
  $ hg commit -m 'win: add b'

  $ hg manifest -v
  644   a
  644   b

  $ hg pull
  pulling from $TESTTMP/unix-repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg manifest -v -r tip
  755 * a

Simulate a Windows merge:

  $ hg --config extensions.n=$TESTTMP/noexec.py merge --debug
    searching for copies back to rev 1
    unmatched files in local:
     b
  resolving manifests
   overwrite None partial False
   ancestor a03b0deabf2b local d6fa54f68ae1+ remote 2d8bcf2dda39
   a: update permissions -> e
  updating: a 1/1 files (100.00%)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Simulate a Windows commit:

  $ hg --config extensions.n=$TESTTMP/noexec.py commit -m 'win: merge'

  $ hg manifest -v
  755 * a
  644   b
