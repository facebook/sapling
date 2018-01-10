Copy of test-dirstate-backup.t for treedirstate

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > treedirstate=
  > [treedirstate]
  > useinnewrepos=True
  > EOF

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

  $ ls .hg/dirstate* | sort
  .hg/dirstate
  .hg/dirstate.tree.* (glob)

