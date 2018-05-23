Quote from test-revert.t but this version is stronger - mtime was changed
manually.

> Test that files reverted to other than the parent are treated as
> "modified", even if none of mode, size and timestamp of it isn't
> changed on the filesystem (see also issue4583).

  $ newrepo
  $ drawdag <<EOS
  > B # B/A=B
  > | # A/A=A
  > A
  > EOS

  $ hg up -q $B

Initially, dirstate does not have mtime set for files

  $ hg debugdirstate
  n 644          1 unset               A
  n 644          1 unset               B

Calling hg status would update mtimes in dirstate (unless mtime == now)

  $ touch -t 200001010000 A B
  $ hg status
  $ hg debugdirstate
  n 644          1 2000-01-01 00:00:00 A
  n 644          1 2000-01-01 00:00:00 B

Revert "A" so its content will change. The size does not change and we set
mtime to make it unchanged.

  $ hg revert -r $A A
  $ touch -t 200001010000 A

BUG: dirstate mtime for "A" should probably be "unset"

  $ hg debugdirstate
  n 644          1 2000-01-01 00:00:00 A
  n 644          1 2000-01-01 00:00:00 B

BUG: due to the bug above, status cannot detect "A" is modified.

  $ hg status
