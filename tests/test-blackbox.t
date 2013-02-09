setup
  $ cat > mock.py <<EOF
  > from mercurial import util
  > import getpass
  > 
  > def makedate():
  >     return 0, 0
  > def getuser():
  >     return 'bob'
  > # mock the date and user apis so the output is always the same
  > def uisetup(ui):
  >     util.makedate = makedate
  >     getpass.getuser = getuser
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > blackbox=
  > mock=`pwd`/mock.py
  > EOF
  $ hg init blackboxtest
  $ cd blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox
  1970/01/01 00:00:00 bob> add a
  1970/01/01 00:00:00 bob> add exited 0 after * seconds (glob)

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'eol=' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'update = echo hooked' >> .hg/hgrc
  $ hg update
  hooked
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg blackbox -l 4
  1970/01/01 00:00:00 bob> update
  1970/01/01 00:00:00 bob> pythonhook-preupdate: hgext.eol.preupdate finished in * seconds (glob)
  1970/01/01 00:00:00 bob> exthook-update: echo hooked finished in * seconds (glob)
  1970/01/01 00:00:00 bob> update exited False after * seconds (glob)

incoming change tracking

  $ hg clone . ../blackboxtest2
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg commit -ma
  $ cd ../blackboxtest2
  $ hg pull
  pulling from $TESTTMP/blackboxtest
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg blackbox -l 3
  1970/01/01 00:00:00 bob> pull
  1970/01/01 00:00:00 bob> 1 incoming changes - new heads: cb9a9f314b8b (glob)
  1970/01/01 00:00:00 bob> pull exited None after * seconds (glob)

cleanup
  $ cd ..
