#require no-eden

First test uncommited changes that should not conflict
  $ newclientrepo <<EOF
  > C
  > |
  > B
  > |
  > A
  > EOF
  $ hg go -q $C
  $ echo added > added
  $ hg add added
  $ echo modifiy > B
  $ hg rm A
  $ touch untracked

  $ hg st
  M B
  A added
  R A
  ? untracked

  $ hg go -q $B

  $ hg st
  M B
  A added
  R A
  ? untracked

Then test --clean works for different types of conflicts
  $ newclientrepo <<EOF
  >    # C/added2 = added2
  >    # C/added1 = added1
  > C  # C/changed = changed
  > |  # C/removed = (removed)
  > B
  > |
  > A  # A/removed =
  >    # A/changed =
  > EOF
  $ hg go -q $B
  $ echo conflict > added1
  $ echo conflict > added2
  $ hg add added2
  $ echo conflict > changed
  $ echo conflict > removed
  $ echo leaveme > added3
  $ hg add added3

  $ hg st
  M removed
  A added2
  A added3
  ? added1
  ? changed

  $ hg go -q $C
  abort: 4 conflicting file changes:
   added1
   added2
   changed
   removed
  (commit, shelve, goto --clean to discard all your changes, or goto --merge to merge them)
  [255]

  $ hg go -q -C $C

  $ hg st
  ? added3
