  $ hgcommit() {
  >    hg commit -u user "$@"
  > }

  $ hg init clhead
  $ cd clhead

  $ touch foo && hg add && hgcommit -m 'foo'
  adding foo
  $ touch bar && hg add && hgcommit -m 'bar'
  adding bar
  $ touch baz && hg add && hgcommit -m 'baz'
  adding baz

  $ echo "flub" > foo
  $ hgcommit -m "flub"
  $ echo "nub" > foo
  $ hgcommit -m "nub"

  $ hg up -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo "c1" > c1
  $ hg add c1
  $ hgcommit -m "c1"
  created new head
  $ echo "c2" > c1
  $ hgcommit -m "c2"

  $ hg up -C 2
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo "d1" > d1
  $ hg add d1
  $ hgcommit -m "d1"
  created new head
  $ echo "d2" > d1
  $ hgcommit -m "d2"
  $ hg tag -l good

fail with three heads
  $ hg up -C good
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  abort: branch 'default' has 3 heads - please merge with an explicit rev
  (run 'hg heads .' to see heads)
  [255]

close one of the heads
  $ hg up -C 6
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hgcommit -m 'close this head' --close-branch

succeed with two open heads
  $ hg up -C good
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -C good
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hgcommit -m 'merged heads'

hg update -C 8
  $ hg update -C 8
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

hg branch some-branch
  $ hg branch some-branch
  marked working directory as branch some-branch
  (branches are permanent and global, did you want a bookmark?)
hg commit
  $ hgcommit -m 'started some-branch'
hg commit --close-branch
  $ hgcommit --close-branch -m 'closed some-branch'

hg update default
  $ hg update default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
hg merge some-branch
  $ hg merge some-branch
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
hg commit (no reopening of some-branch)
  $ hgcommit -m 'merge with closed branch'

  $ cd ..
