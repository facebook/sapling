#require no-fsmonitor no-eden

setup
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
  $ sl add a
  $ sl blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'
  [legacy][command] add a
  [legacy][command_finish] add a exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish"]}}'

FIXME: (recursive) alias expansion is not logged
  $ rm -rf ./.sl/blackbox*
  $ sl so-confusing
  $ sl blackbox --pattern '{"start": "_"}' | sort -u
  [command] [*, "blackbox", "--pattern", "{\"start\": \"_\"}"] * (glob)
  [command] [*, "so-confusing"] * (glob)

incoming change tracking

create two heads to verify that we only see one change in the log later
  $ sl commit -ma
  $ sl push -q -r . --to head1 --create
  $ sl up null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ sl commit -Amb
  adding b

clone, commit, pull
  $ sl push -q -r . --to head2 --create
  $ newclientrepo blackboxtest2 blackboxtest_server head1 head2
  $ cd ../blackboxtest
  $ echo c > c
  $ sl commit -Amc
  adding c
  $ sl push -q -r . --to head2
  $ cd ../blackboxtest2
  $ sl pull
  pulling from test:blackboxtest_server
  searching for changes
  $ sl blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'
  [legacy][command] pull -q -B head1
  [legacy][command_finish] pull -q -B head1 exited 0 after 0.00 seconds
  [legacy][command] pull -q -B head2
  [legacy][command_finish] pull -q -B head2 exited 0 after 0.00 seconds
  [legacy][command] pull
  [legacy][command_finish] pull exited 0 after 0.00 seconds
  [legacy][command] blackbox --pattern '{"legacy_log":{"service":["or","command","command_finish","command_alias"]}}'

we must not cause a failure if we cannot write to the log

  $ rm -rf .sl/blackbox*
  $ mkdir -p .sl/blackbox
  $ touch .sl/blackbox/v1
  $ sl pull
  pulling from test:blackboxtest_server

  $ rm .sl/blackbox/v1

extension and python hooks - use the eol extension for a pythonhook

  $ echo '[extensions]' >> .sl/config
  $ echo 'eol=' >> .sl/config
  $ echo '[hooks]' >> .sl/config
  $ echo 'update = echo hooked' >> .sl/config
  $ sl goto tip
  hooked
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat >> .sl/config <<EOF
  > [extensions]
  > # disable eol, because it is not needed for subsequent tests
  > # (in addition, keeping it requires extra care for fsmonitor)
  > eol=!
  > EOF
  $ sl blackbox --pattern '{"blocked":{"op":["or","pythonhook","exthook"]}}'

log rotation (tested in the Rust land)

blackbox does not crash with empty log message

  $ newclientrepo
  $ cat > $TESTTMP/uilog.py << EOF
  > from __future__ import absolute_import
  > from sapling import registrar, scmutil, util
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
  $ sl uilog foo
  $ sl uilog foo ''

blackbox adds "\n" automatically

  $ setconfig blackbox.track=bar
  $ sl uilog bar bar1-NEWLINE
  $ sl uilog bar bar2
  $ sl uilog bar bar3
  $ sl blackbox --pattern '{"legacy_log":{"service":"bar"}}'
  [legacy][bar] bar1
  [legacy][bar] bar2
  [legacy][bar] bar3

blackbox can log without a ui object using util.log

  $ setconfig blackbox.track=withoutui
  $ sl utillog withoutui "this log is without a ui"
  $ sl blackbox --pattern '{"legacy_log":{"service":"withoutui"}}'
  [legacy][withoutui] this log is without a ui

blackbox writes Request ID if HGREQUESTID is set
(This is not implemented in the new blackbox.  Maybe it is not that important nowadays?)

Test EDENSCM_BLACKBOX_TAGS

  $ EDENSCM_BLACKBOX_TAGS='foo bar' sl root
  $TESTTMP/repo1
  $ sl blackbox -p '{"tags": "_"}'
  [tags] foo, bar
  [tags] foo, bar (?)
  $ sl blackbox -p '{"tags": {"names": ["contain", "bar"]}}'
  [tags] foo, bar
  [tags] foo, bar (?)

blackbox should not fail with "TypeError: not enough arguments for format string"

  $ rm -rf ./.sl/blackbox*
  $ sl debugshell --command "ui.log('foo', 'ba' + 'r %s %r')"
  $ sl debugshell --command "ui.log('foo', 'ba' + 'r %s %r', 'arg1')"
  $ sl blackbox --pattern '{"legacy_log":{"service":"foo"}}'
  [legacy][foo] bar %s %r
  [legacy][foo] bar %s %r arg1

blackbox.enable=false skips opening the on-disk log, even for read-write
commands (checked via the file, since in-process tests share the memory buffer)

  $ newclientrepo enabletest
  $ echo x > x
  $ rm -rf .sl/blackbox
  $ sl --config blackbox.enable=false add x
  $ test -e .sl/blackbox
  [1]

logging stays on by default

  $ echo y > y
  $ sl add y
  $ test -e .sl/blackbox
