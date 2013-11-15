setup
  $ cat > mock.py <<EOF
  > from mercurial import util
  > 
  > def makedate():
  >     return 0, 0
  > def getuser():
  >     return 'bob'
  > # mock the date and user apis so the output is always the same
  > def uisetup(ui):
  >     util.makedate = makedate
  >     util.getuser = getuser
  > EOF
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > blackbox=
  > mock=`pwd`/mock.py
  > mq=
  > EOF
  $ hg init blackboxtest
  $ cd blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox
  1970/01/01 00:00:00 bob> add a
  1970/01/01 00:00:00 bob> add a exited 0 after * seconds (glob)

incoming change tracking

create two heads to verify that we only see one change in the log later
  $ hg commit -ma
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg commit -Amb
  adding b
  created new head

clone, commit, pull
  $ hg clone . ../blackboxtest2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c > c
  $ hg commit -Amc
  adding c
  $ cd ../blackboxtest2
  $ hg pull
  pulling from $TESTTMP/blackboxtest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg blackbox -l 3
  1970/01/01 00:00:00 bob> pull
  1970/01/01 00:00:00 bob> 1 incoming changes - new heads: d02f48003e62
  1970/01/01 00:00:00 bob> pull exited 0 after * seconds (glob)

we must not cause a failure if we cannot write to the log

  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)

#if unix-permissions no-root
  $ chmod 000 .hg/blackbox.log
  $ hg --debug incoming
  warning: cannot write to blackbox.log: Permission denied
  comparing with $TESTTMP/blackboxtest (glob)
  query 1; heads
  searching for changes
  all local heads known remotely
  changeset:   2:d02f48003e62c24e2659d97d30f2a83abe5d5d51
  tag:         tip
  phase:       draft
  parent:      1:6563da9dcf87b1949716e38ff3e3dfaa3198eb06
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    2:ab9d46b053ebf45b7996f2922b9893ff4b63d892
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      c
  extra:       branch=default
  description:
  c
  
  
#endif
  $ hg pull
  pulling from $TESTTMP/blackboxtest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

a failure reading from the log is fine
#if unix-permissions no-root
  $ hg blackbox -l 3
  abort: Permission denied: $TESTTMP/blackboxtest2/.hg/blackbox.log
  [255]

  $ chmod 600 .hg/blackbox.log
#endif

backup bundles get logged

  $ touch d
  $ hg commit -Amd
  adding d
  created new head
  $ hg strip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/*-backup.hg (glob)
  $ hg blackbox -l 3
  1970/01/01 00:00:00 bob> strip tip
  1970/01/01 00:00:00 bob> saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/*-backup.hg (glob)
  1970/01/01 00:00:00 bob> strip tip exited 0 after * seconds (glob)

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'eol=' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'update = echo hooked' >> .hg/hgrc
  $ hg update
  hooked
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg blackbox -l 4
  1970/01/01 00:00:00 bob> update
  1970/01/01 00:00:00 bob> pythonhook-preupdate: hgext.eol.preupdate finished in * seconds (glob)
  1970/01/01 00:00:00 bob> exthook-update: echo hooked finished in * seconds (glob)
  1970/01/01 00:00:00 bob> update exited 0 after * seconds (glob)

log rotation

  $ echo '[blackbox]' >> .hg/hgrc
  $ echo 'maxsize = 20 b' >> .hg/hgrc
  $ echo 'maxfiles = 3' >> .hg/hgrc
  $ hg status
  $ hg status
  $ hg status
  $ hg tip -q
  2:d02f48003e62
  $ ls .hg/blackbox.log*
  .hg/blackbox.log
  .hg/blackbox.log.1
  .hg/blackbox.log.2

cleanup
  $ cd ..
