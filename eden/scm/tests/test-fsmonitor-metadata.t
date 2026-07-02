#require fsmonitor no-eden


  $ newclientrepo
  $ echo foo > foo
  $ sl commit -qAm foo
  $ sl status
  $ echo banana > foo
  $ LOG=vfs=trace sl status
  M foo

  $ sl dbsh << 'EOS'
  > watchman_command = repo._watchmanclient.command
  > # Simulate watchman restart
  > watchman_command('watch-del-all')
  > EOS

  $ LOG=vfs=trace sl status
  M foo
