#require no-fsmonitor
#debugruntest-compatible

setup
  $ configure modernclient
  $ readconfig <<EOF
  > [alias]
  > blackbox = blackbox --no-timestamp --no-sid
  > confuse = log --limit 3
  > so-confusing = confuse --style compact
  > EOF
  $ setconfig tracing.threshold=100000
  $ newclientrepo blackboxtest

command, exit codes, and duration

  $ echo a > a
  $ hg add a
  $ hg blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'
  [legacy][command] up -q tip
  [legacy][command_finish] up -q tip exited 0 after 0.00 seconds
  [legacy][command] add a
  [legacy][command_finish] add a exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'

alias expansion is logged
  $ rm -rf ./.hg/blackbox*
  $ hg confuse
  $ hg blackbox
  [command] [*, "confuse"] started by uid 0 as pid 0 with nice 0 (glob)
  [process_tree] (this process)
  [command] [*, "confuse"] started by uid 0 as pid 0 with nice 0 (glob) (?)
  [process_tree] (this process) (?)
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command_info] (?)
  [legacy][env_vars] (?)
  [legacy][command] confuse
  [legacy][dirstate_info]
  [legacy][jobid]
  [legacy][changelog_info]
  [legacy][visibility] read 0 heads:
  [legacy][dirstate_info]
  [legacy][command_finish] confuse exited 0 after 0.00 seconds
  [legacy][connectionpool]
  [legacy][command_info]
  [commmand_finish] exited 0 in 0 ms, max RSS: 0 bytes
  [tracing] (binary data of * bytes) (glob)
  [command] [*, "blackbox"] started by uid 0 as pid 0 with nice 0 (glob)
  [process_tree] (this process)
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command] blackbox
  [legacy][dirstate_info]
  [legacy][jobid]

recursive aliases work correctly
  $ rm -rf ./.hg/blackbox*
  $ hg so-confusing
  $ hg blackbox
  [command] [*, "so-confusing"] started by uid 0 as pid 0 with nice 0 (glob)
  [process_tree] (this process)
  [command] [*, "so-confusing"] started by uid 0 as pid 0 with nice 0 (glob) (?)
  [process_tree] (this process) (?)
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command_info] (?)
  [legacy][env_vars] (?)
  [legacy][command] so-confusing
  [legacy][dirstate_info]
  [legacy][jobid]
  [legacy][changelog_info]
  [legacy][visibility] read 0 heads:
  [legacy][dirstate_info]
  [legacy][command_finish] so-confusing exited 0 after 0.00 seconds
  [legacy][connectionpool]
  [legacy][command_info]
  [commmand_finish] exited 0 in 0 ms, max RSS: 0 bytes
  [tracing] (binary data of * bytes) (glob)
  [command] [*, "blackbox"] started by uid 0 as pid 0 with nice 0 (glob)
  [process_tree] (this process)
  [legacy][command_info]
  [legacy][env_vars]
  [legacy][command] blackbox
  [legacy][dirstate_info]
  [legacy][jobid]

incoming change tracking

create two heads to verify that we only see one change in the log later
  $ hg commit -ma
  $ hg push -q -r . --to head1 --create
  $ hg up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg commit -Amb
  adding b

clone, commit, pull
  $ hg push -q -r . --to head2 --create
  $ newclientrepo blackboxtest2 test:blackboxtest_server head1 head2
  $ cd ../blackboxtest
  $ echo c > c
  $ hg commit -Amc
  adding c
  $ hg push -q -r . --to head2
  $ cd ../blackboxtest2
  $ hg pull
  pulling from test:blackboxtest_server
  searching for changes
  $ hg blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'
  [legacy][command] pull -q -B head1
  [legacy][command_finish] pull -q -B head1 exited 0 after 0.00 seconds
  [legacy][command] pull -q -B head2
  [legacy][command_finish] pull -q -B head2 exited 0 after 0.00 seconds
  [legacy][command] up -q tip
  [legacy][command_finish] up -q tip exited 0 after 0.00 seconds
  [legacy][command] pull
  [legacy][command_finish] pull exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'

we must not cause a failure if we cannot write to the log

  $ rm -rf .hg/blackbox*
  $ mkdir -p .hg/blackbox
  $ touch .hg/blackbox/v1
  $ hg pull
  pulling from test:blackboxtest_server

  $ rm .hg/blackbox/v1

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .hg/hgrc
  $ echo 'eol=' >> .hg/hgrc
  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'update = echo hooked' >> .hg/hgrc
  $ hg goto
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

log rotation (tested in the Rust land)

blackbox does not crash with empty log message

  $ newclientrepo
  $ cat > $TESTTMP/uilog.py << EOF
  > from __future__ import absolute_import
  > from edenscm import registrar, scmutil, util
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

Test EDENSCM_BLACKBOX_TAGS

  $ EDENSCM_BLACKBOX_TAGS='foo bar' hg root
  $TESTTMP/repo1
  $ hg blackbox -p '{"tags": "_"}'
  [tags] foo, bar
  [tags] foo, bar (?)
  $ hg blackbox -p '{"tags": {"names": ["contain", "bar"]}}'
  [tags] foo, bar
  [tags] foo, bar (?)

blackbox should not fail with "TypeError: not enough arguments for format string"

  $ rm -rf ./.hg/blackbox*
  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r')"
  $ hg debugshell --command "ui.log('foo', 'ba' + 'r %s %r', 'arg1')"
  $ hg blackbox --pattern '{"legacy_log":{"service":"foo"}}'
  [legacy][foo] bar %s %r
  [legacy][foo] bar %s %r arg1
