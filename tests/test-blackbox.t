setup
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > blackbox=
  > mock=$TESTDIR/mockblackbox.py
  > mq=
  > EOF
  $ hg init blackboxtest
  $ cd blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox --config blackbox.dirty=True
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> add a
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> add a exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000+ (5000)> blackbox

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
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> pull
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> updated served branch cache in * seconds (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> wrote served branch cache with 1 labels and 2 nodes
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> 1 incoming changes - new heads: d02f48003e62
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> pull exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> blackbox -l 6

we must not cause a failure if we cannot write to the log

  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)

  $ mv .hg/blackbox.log .hg/blackbox.log-
  $ mkdir .hg/blackbox.log
  $ hg --debug incoming
  warning: cannot write to blackbox.log: * (glob)
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
  
  
  $ hg pull
  pulling from $TESTTMP/blackboxtest (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

a failure reading from the log is fatal

  $ hg blackbox -l 3
  abort: *$TESTTMP/blackboxtest2/.hg/blackbox.log* (glob)
  [255]

  $ rmdir .hg/blackbox.log
  $ mv .hg/blackbox.log- .hg/blackbox.log

backup bundles get logged

  $ touch d
  $ hg commit -Amd
  adding d
  created new head
  $ hg strip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/*-backup.hg (glob)
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @73f6ee326b27d820b0472f1a825e3a50f3dc489b (5000)> strip tip
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/73f6ee326b27-7612e004-backup.hg (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> updated base branch cache in * seconds (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> wrote base branch cache with 1 labels and 2 nodes
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> strip tip exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> blackbox -l 6

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'eol=' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'update = echo hooked' >> .hg/hgrc
  $ hg update
  hooked
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 other heads for branch "default"
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> update
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> writing .hg/cache/tags2-visible with 0 tags
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> pythonhook-preupdate: hgext.eol.preupdate finished in * seconds (glob)
  1970/01/01 00:00:00 bob @d02f48003e62c24e2659d97d30f2a83abe5d5d51 (5000)> exthook-update: echo hooked finished in * seconds (glob)
  1970/01/01 00:00:00 bob @d02f48003e62c24e2659d97d30f2a83abe5d5d51 (5000)> update exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @d02f48003e62c24e2659d97d30f2a83abe5d5d51 (5000)> blackbox -l 6

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
  $ cd ..

  $ hg init blackboxtest3
  $ cd blackboxtest3
  $ hg blackbox
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox
  $ mv .hg/blackbox.log .hg/blackbox.log-
  $ mkdir .hg/blackbox.log
  $ sed -e 's/\(.*test1.*\)/#\1/; s#\(.*commit2.*\)#os.rmdir(".hg/blackbox.log")\
  > os.rename(".hg/blackbox.log-", ".hg/blackbox.log")\
  > \1#' $TESTDIR/test-dispatch.py > ../test-dispatch.py
  $ python $TESTDIR/blackbox-readonly-dispatch.py
  running: add foo
  result: 0
  running: commit -m commit1 -d 2000-01-01 foo
  result: None
  running: commit -m commit2 -d 2000-01-02 foo
  result: None
  running: log -r 0
  changeset:   0:0e4634943879
  user:        test
  date:        Sat Jan 01 00:00:00 2000 +0000
  summary:     commit1
  
  result: None
  running: log -r tip
  changeset:   1:45589e459b2e
  tag:         tip
  user:        test
  date:        Sun Jan 02 00:00:00 2000 +0000
  summary:     commit2
  
  result: None
  $ hg blackbox
  1970/01/01 00:00:00 bob @0e46349438790c460c5c9f7546bfcd39b267bbd2 (5000)> commit -m commit2 -d 2000-01-02 foo
  1970/01/01 00:00:00 bob @0e46349438790c460c5c9f7546bfcd39b267bbd2 (5000)> updated served branch cache in * seconds (glob)
  1970/01/01 00:00:00 bob @0e46349438790c460c5c9f7546bfcd39b267bbd2 (5000)> wrote served branch cache with 1 labels and 1 nodes
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> commit -m commit2 -d 2000-01-02 foo exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r 0
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> writing .hg/cache/tags2-visible with 0 tags
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r 0 exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r tip
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r tip exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> blackbox

Test log recursion from dirty status check

  $ cat > ../r.py <<EOF
  > from mercurial import context, error, extensions
  > x=[False]
  > def status(orig, *args, **opts):
  >     args[0].repo().ui.log("broken", "recursion?")
  >     return orig(*args, **opts)
  > def reposetup(ui, repo):
  >     extensions.wrapfunction(context.basectx, 'status', status)
  > EOF
  $ hg id --config extensions.x=../r.py --config blackbox.dirty=True
  45589e459b2e tip

cleanup
  $ cd ..
