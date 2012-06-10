Corrupt an hg repo with two pulls.
create one repo with a long history

  $ hg init source1
  $ cd source1
  $ touch foo
  $ hg add foo
  $ for i in 1 2 3 4 5 6 7 8 9 10; do
  >     echo $i >> foo
  >     hg ci -m $i
  > done
  $ cd ..

create one repo with a shorter history

  $ hg clone -r 0 source1 source2
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd source2
  $ echo a >> foo
  $ hg ci -m a
  $ cd ..

create a third repo to pull both other repos into it

  $ hg init corrupted
  $ cd corrupted

use a hook to make the second pull start while the first one is still running

  $ echo '[hooks]' >> .hg/hgrc
  $ echo 'prechangegroup = sleep 5' >> .hg/hgrc

start a pull...

  $ hg pull ../source1 > pull.out 2>&1 &

... and start another pull before the first one has finished

  $ sleep 1
  $ hg pull ../source2 2>/dev/null
  pulling from ../source2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ cat pull.out
  pulling from ../source1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 10 changesets with 10 changes to 1 files
  (run 'hg update' to get a working copy)

see the result

  $ wait
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 11 changesets, 11 total revisions

  $ cd ..
