#debugruntest-compatible
#require fsmonitor

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

This is the code under test - we need to notice things were deleted while watchman wasn't running.
  $ hg status
  M modified
  A added
  R removed
  ! missing
  $ hg debugtree list
  .gitignore: 0100644 8 * EXIST_P1 EXIST_NEXT * (glob)
  added: 00 -1 * EXIST_NEXT NEED_CHECK  (glob)
  missing: 0100644 0 * EXIST_P1 EXIST_NEXT NEED_CHECK  (glob)
  modified: 0100644 0 * EXIST_P1 EXIST_NEXT NEED_CHECK  (glob)
  removed: 00 0 * EXIST_P1 NEED_CHECK  (glob)
