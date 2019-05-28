  $ setconfig extensions.treemanifest=!
#require no-fsmonitor

setup
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > blackbox=
  > mock=$TESTDIR/mockblackbox.py
  > [alias]
  > confuse = log --limit 3
  > so-confusing = confuse --style compact
  > EOF
  $ setconfig tracing.threshold=100000
  $ hg init blackboxtest
  $ cd blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox --config blackbox.dirty=True
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> init blackboxtest exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> add a
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> add a exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000+ (5000)> blackbox --config *blackbox.dirty=True* (glob)

alias expansion is logged
  $ rm ./.hg/blackbox.log
  $ hg confuse
  $ hg blackbox
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> confuse
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> alias 'confuse' expands to 'log --limit 3'
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> confuse exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox

recursive aliases work correctly
  $ rm ./.hg/blackbox.log
  $ hg so-confusing
  $ hg blackbox
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> so-confusing
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> alias 'so-confusing' expands to 'confuse --style compact'
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> alias 'confuse' expands to 'log --limit 3'
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> so-confusing exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox

incoming change tracking

create two heads to verify that we only see one change in the log later
  $ hg commit -ma
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg commit -Amb
  adding b

clone, commit, pull
  $ hg clone . ../blackboxtest2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c > c
  $ hg commit -Amc
  adding c
  $ cd ../blackboxtest2
  $ hg pull
  pulling from $TESTTMP/blackboxtest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d02f48003e62
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> perftweaks updated served branch cache
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> clone . ../blackboxtest2 exited 0 after 0.00 seconds
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> pull
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
  comparing with $TESTTMP/blackboxtest
  query 1; heads
  searching for changes
  all local heads known remotely
  changeset:   2:d02f48003e62c24e2659d97d30f2a83abe5d5d51
  tag:         tip
  phase:       draft
  parent:      1:6563da9dcf87b1949716e38ff3e3dfaa3198eb06
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    ab9d46b053ebf45b7996f2922b9893ff4b63d892
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      c
  extra:       branch=default
  description:
  c
  
  
  $ hg pull
  pulling from $TESTTMP/blackboxtest
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d02f48003e62

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
  $ hg debugstrip tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/*-backup.hg (glob)
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> commit -Amd
  1970/01/01 00:00:00 bob @73f6ee326b27d820b0472f1a825e3a50f3dc489b (5000)> commit -Amd exited 0 after 0.00 seconds
  1970/01/01 00:00:00 bob @73f6ee326b27d820b0472f1a825e3a50f3dc489b (5000)> debugstrip tip
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> saved backup bundle to $TESTTMP/blackboxtest2/.hg/strip-backup/73f6ee326b27-7612e004-backup.hg
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> debugstrip tip exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> blackbox -l 6

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'eol=' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'update = echo hooked' >> .hg/hgrc
  $ hg update
  The fsmonitor extension is incompatible with the eol extension and has been disabled. (fsmonitor !)
  hooked
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  updated to "d02f48003e62: c"
  1 other heads for branch "default"
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > # disable eol, because it is not needed for subsequent tests
  > # (in addition, keeping it requires extra care for fsmonitor)
  > eol=!
  > EOF
  $ hg blackbox -l 6
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> perftweaks updated visible branch cache
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> writing .hg/cache/tags2-visible with 0 tags
  1970/01/01 00:00:00 bob @6563da9dcf87b1949716e38ff3e3dfaa3198eb06 (5000)> pythonhook-preupdate: *hgext.eol.preupdate finished in * seconds (glob)
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
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> init blackboxtest3 exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox
  $ mv .hg/blackbox.log .hg/blackbox.log-
  $ mkdir .hg/blackbox.log
  $ sed -e 's/\(.*test1.*\)/#\1/; s#\(.*commit2.*\)#os.rmdir(".hg/blackbox.log")\
  > os.rename(".hg/blackbox.log-", ".hg/blackbox.log")\
  > \1#' $TESTDIR/test-dispatch.py > ../test-dispatch.py
  $ $PYTHON $TESTDIR/blackbox-readonly-dispatch.py
  running: add foo
  result: 0
  running: commit -m commit1 -d 2000-01-01 foo
  result: 0
  running: commit -m commit2 -d 2000-01-02 foo
  result: 0
  running: log -r 0
  changeset:   0:0e4634943879
  user:        test
  date:        Sat Jan 01 00:00:00 2000 +0000
  summary:     commit1
  
  result: 0
  running: log -r tip
  changeset:   1:45589e459b2e
  tag:         tip
  user:        test
  date:        Sun Jan 02 00:00:00 2000 +0000
  summary:     commit2
  
  result: 0
  $ hg blackbox
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> blackbox exited 0 after 0.00 seconds
  1970/01/01 00:00:00 bob @0e46349438790c460c5c9f7546bfcd39b267bbd2 (5000)> commit -m commit2 -d 2000-01-02 foo
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> commit -m commit2 -d 2000-01-02 foo exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r 0
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> writing .hg/cache/tags2-visible with 0 tags
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r 0 exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r tip
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> log -r tip exited 0 after * seconds (glob)
  1970/01/01 00:00:00 bob @45589e459b2edfbf3dbde7e01f611d2c1e7453d7 (5000)> blackbox

Test log recursion from dirty status check

  $ cat > ../r.py <<EOF
  > from edenscm.mercurial import context, error, extensions
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

#if chg

when using chg, blackbox.log should get rotated correctly

  $ cat > $TESTTMP/noop.py << EOF
  > from __future__ import absolute_import
  > import time
  > from edenscm.mercurial import registrar, scmutil
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('noop')
  > def noop(ui, repo):
  >     pass
  > EOF

  $ hg init blackbox-chg
  $ cd blackbox-chg

  $ cat > .hg/hgrc << EOF
  > [blackbox]
  > maxsize = 500B
  > [extensions]
  > # extension change forces chg to restart
  > noop=$TESTTMP/noop.py
  > EOF

  $ $PYTHON -c 'print("a" * 400)' > .hg/blackbox.log
  $ chg noop
  $ chg noop
  $ chg noop
  $ chg noop
  $ chg noop

  $ cat > showsize.py << 'EOF'
  > import os, sys
  > limit = 500
  > for p in sys.argv[1:]:
  >     size = os.stat(p).st_size
  >     if size >= limit:
  >         desc = '>='
  >     else:
  >         desc = '<'
  >     print('%s: %s %d' % (p, desc, limit))
  > EOF

  $ $PYTHON showsize.py .hg/blackbox*
  .hg/blackbox.log: < 500
  .hg/blackbox.log.1: >= 500
  .hg/blackbox.log.2: >= 500

  $ cd ..

With chg, blackbox should not create the log file if the repo is gone

  $ hg init repo0
  $ hg --config extensions.a=! -R repo0 log
  $ rm -rf $TESTTMP/repo0
  $ hg --config extensions.a=! init repo0

#endif

blackbox should work if repo.ui.log is not called (issue5518)

  $ cat > $TESTTMP/raise.py << EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import registrar, scmutil
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('raise')
  > def raisecmd(*args):
  >     raise RuntimeError('raise')
  > EOF

  $ cat >> $HGRCPATH << EOF
  > [blackbox]
  > track = command_exception
  > [extensions]
  > raise=$TESTTMP/raise.py
  > EOF

  $ hg init $TESTTMP/blackbox-exception-only
  $ cd $TESTTMP/blackbox-exception-only

#if chg
 (chg exits 255 because it fails to receive an exit code)
  $ hg raise 2>/dev/null
  [1]
#else
 (hg exits 1 because Python default exit code for uncaught exception is 1)
  $ hg raise 2>/dev/null
  [1]
#endif

  $ head -1 .hg/blackbox.log
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> ** Mercurial Distributed SCM (version *) has crashed: (glob)
  $ tail -2 .hg/blackbox.log
  RuntimeError: raise
  
blackbox does not crash with empty log message

  $ newrepo
  $ cat > $TESTTMP/uilog.py << EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import registrar, scmutil, util
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('uilog')
  > def uilogcmd(ui, repo, category, *args):
  >     args = [a.replace('-NEWLINE', '\n') for a in args]
  >     ui.log(category, *args)
  > @command('utillog')
  > def utillogcmd(ui, repo, category, *args):
  >     util.log(category, *args)
  > EOF
  $ setconfig extensions.uilog=$TESTTMP/uilog.py
  $ setconfig blackbox.track=foo
  $ hg uilog foo
  $ hg uilog foo ''
  $ hg blackbox | grep foo
  [1]

blackbox adds "\n" automatically

  $ setconfig blackbox.track=bar
  $ hg uilog bar bar1-NEWLINE
  $ hg uilog bar bar2
  $ hg uilog bar bar3
  $ hg blackbox | grep bar
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> bar1
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> bar2
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> bar3

blackbox can log without a ui object using util.log

  $ setconfig blackbox.track=withoutui
  $ hg utillog withoutui "this log is without a ui"
  $ hg blackbox | grep without
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> this log is without a ui

blackbox writes Request ID if HGREQUESTID is set

  $ newrepo
  $ setconfig blackbox.track=*
  $ HGREQUESTID=aabea740-97d8-49e5-8453-14e5ae3d4099 hg root >/dev/null
  $ hg blackbox | grep aabea740-97d8-49e5-8453-14e5ae3d4099
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)[aabea740-97d8-49e5-8453-14e5ae3d4099]> root

blackbox should not fail with "TypeError: not enough arguments for format string"

  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r')"
  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r', 'arg1')"
  $ hg blackbox | grep bar
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> bar %s %r
  1970/01/01 00:00:00 bob @0000000000000000000000000000000000000000 (5000)> bar %s %r arg1

