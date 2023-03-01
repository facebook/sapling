#debugruntest-compatible
#require fsmonitor

  $ setconfig status.use-rust=true workingcopy.use-rust=true workingcopy.ruststatus=false

  $ configure modernclient
  $ newclientrepo

  $ echo ignored > .gitignore
  $ touch ignored missing removed modified
  $ hg commit -Aqm foo

  $ touch untracked added
  $ hg add added
  $ hg rm removed

  $ hg status
  A added
  R removed
  ? untracked

  $ hg dbsh << 'EOS'
  > watchman_command = repo._watchmanclient.command
  > watchman_command('watch-del-all')
  > EOS

  $ rm ignored missing untracked
  $ echo foo > modified

XXX fixme - this should report "missing" as "!"
  $ hg status
  M modified
  A added
  R removed
  $ hg debugtree list
  .gitignore: 0100644 8 * EXIST_P1 EXIST_NEXT  (glob)
  added: 00 -1 * EXIST_NEXT NEED_CHECK  (glob)
  ignored: 0666 -1 * NEED_CHECK  (glob)
  missing: 0100644 0 * EXIST_P1 EXIST_NEXT  (glob)
  modified: 0100644 0 * EXIST_P1 EXIST_NEXT NEED_CHECK  (glob)
  removed: 00 0 * EXIST_P1 NEED_CHECK  (glob)
  untracked: 0666 -1 *  (glob)
