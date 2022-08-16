#chg-compatible
#debugruntest-compatible

Set up

  $ hg init repo
  $ cd repo

Try to import an empty patch

  $ hg import --no-commit - <<EOF
  > EOF
  applying patch from stdin
  abort: stdin: no diffs found
  [255]

No dirstate backups are left behind

  $ echo .hg/dirstate*
  .hg/dirstate

