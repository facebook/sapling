  $ $PYTHON -c 'import pushrebase' || exit 80

  $ . "$TESTDIR/library.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > pushrebase=
  > [experimental]
  > bundle2lazylocking=True
  > EOF

Test verify sql lock is not held during prelockrebase hook

  $ cat >> $TESTTMP/locktester.py <<EOF
  > import os
  > from mercurial import extensions, bundle2, util
  > def checklock(repo, *args, **kwargs):
  >     if len(repo.heldlocks) > 0:
  >         raise util.Abort("lock was TAKEN")
  >     raise util.Abort("lock was FREE")
  > EOF

  $ initserver master master
  $ cat >> master/.hg/hgrc <<EOF
  > [hooks]
  > prepushrebase=python:$TESTTMP/locktester.py:checklock
  > EOF
  $ cd master
  $ touch a && hg ci -Aqm a
  $ hg book master
  $ cd ..

  $ initclient client
  $ cd client
  $ hg pull -q ssh://user@dummy/master
  $ hg up -q master
  $ touch b && hg ci -Aqm b

  $ hg push ssh://user@dummy/master --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: error: prepushrebase hook failed: lock was FREE
  abort: lock was FREE
  [255]

  $ cd ../master
  $ hg log -T '{rev}\n'
  0
