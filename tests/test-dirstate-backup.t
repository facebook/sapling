Set up

  $ hg init repo
  $ cd repo

Try to import an empty patch

  $ hg import --no-commit - <<EOF
  > EOF
  applying patch from stdin
  abort: stdin: no diffs found
  [255]

A dirstate backup is left behind

  $ ls .hg/dirstate* | sort
  .hg/dirstate
  .hg/dirstate.backup.import.* (glob)

