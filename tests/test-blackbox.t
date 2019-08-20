  $ setconfig extensions.treemanifest=!
#require no-fsmonitor

setup
  $ cat >> $HGRCPATH <<EOF
  > [alias]
  > blackbox = blackbox --no-timestamp --no-sid
  > confuse = log --limit 3
  > so-confusing = confuse --style compact
  > EOF
  $ setconfig tracing.threshold=100000
  $ hg init blackboxtest
  $ cd blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'
  [legacy][command] add a
  [legacy][command_finish] add a exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'

alias expansion is logged
  $ rm -rf ./.hg/blackbox*
  $ hg confuse
  $ hg blackbox
  [legacy][env_vars]
  [legacy][command] confuse
  [legacy][dirstate_info]
  [legacy][jobid]
  [legacy][dirstate_info]
  [legacy][command_finish] confuse exited 0 after 0.00 seconds
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command] blackbox
  [legacy][dirstate_info]
  [legacy][jobid]

recursive aliases work correctly
  $ rm -rf ./.hg/blackbox*
  $ hg so-confusing
  $ hg blackbox
  [legacy][env_vars]
  [legacy][command] so-confusing
  [legacy][dirstate_info]
  [legacy][jobid]
  [legacy][dirstate_info]
  [legacy][command_finish] so-confusing exited 0 after 0.00 seconds
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command] blackbox
  [legacy][dirstate_info]
  [legacy][jobid]

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
  $ hg blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'
  [legacy][command] pull
  [legacy][command_finish] pull exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'

we must not cause a failure if we cannot write to the log

  $ hg rollback
  repository tip rolled back to revision 1 (undo pull)

  $ rm -rf .hg/blackbox*
  $ mkdir -p .hg/blackbox
  $ touch .hg/blackbox/v1
  $ hg --debug incoming
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

  $ rm .hg/blackbox/v1

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
  $ hg blackbox --pattern '{"blocked":{"op":["or","pythonhook","exthook"]}}'
  [blocked] PythonHook (preupdate.eol) blocked for 0 ms
  [blocked] ExtHook (update) blocked for 0 ms

log rotation (tested in the Rust land)

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

blackbox adds "\n" automatically

  $ setconfig blackbox.track=bar
  $ hg uilog bar bar1-NEWLINE
  $ hg uilog bar bar2
  $ hg uilog bar bar3
  $ hg blackbox --pattern '{"legacy_log":{"service":"bar"}}'
  [legacy][bar] bar1
  [legacy][bar] bar2
  [legacy][bar] bar3

blackbox can log without a ui object using util.log

  $ setconfig blackbox.track=withoutui
  $ hg utillog withoutui "this log is without a ui"
  $ hg blackbox --pattern '{"legacy_log":{"service":"withoutui"}}'
  [legacy][withoutui] this log is without a ui

blackbox writes Request ID if HGREQUESTID is set
(This is not implemented in the new blackbox.  Maybe it is not that important nowadays?)

blackbox should not fail with "TypeError: not enough arguments for format string"

  $ rm -rf ./.hg/blackbox*
  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r')"
  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r', 'arg1')"
  $ hg blackbox --pattern '{"legacy_log":{"service":"foo"}}'
  [legacy][foo] bar %s %r
  [legacy][foo] bar %s %r arg1

