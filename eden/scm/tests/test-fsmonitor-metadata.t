#chg-compatible
#require fsmonitor

  $ configure modernclient
  $ setconfig status.use-rust=true workingcopy.use-rust=true

  $ newclientrepo
  $ echo foo > foo
  $ hg commit -qAm foo
  $ hg status
  $ echo banana > foo
  $ LOG=vfs=trace hg status
  M foo

  $ hg dbsh << 'EOS'
  > watchman_command = repo._watchmanclient.command
  > # Simulate watchman restart
  > watchman_command('watch-del-all')
  > EOS

  $ LOG=vfs=trace hg status
  M foo
