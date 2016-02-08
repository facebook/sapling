  $ hg init test
  $ cd test

  $ cat > .hg/hgrc <<EOF
  > [server]
  > validate=1
  > EOF

  $ echo alpha > alpha
  $ echo beta > beta
  $ hg addr
  adding alpha
  adding beta
  $ hg ci -m 1

  $ cd ..
  $ hg clone test test-clone
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test spurious filelog entries:

  $ cd test-clone
  $ echo blah >> beta
  $ cp .hg/store/data/beta.i tmp1
  $ hg ci -m 2
  $ cp .hg/store/data/beta.i tmp2
  $ hg -q rollback
  $ mv tmp2 .hg/store/data/beta.i
  $ echo blah >> beta
  $ hg ci -m '2 (corrupt)'

Expected to fail:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   beta@1: dddc47b3ba30 not in manifests
  2 files, 2 changesets, 4 total revisions
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

  $ hg push
  pushing to $TESTTMP/test (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  transaction abort!
  rollback completed
  abort: received spurious file revlog entry
  [255]

  $ hg -q rollback
  $ mv tmp1 .hg/store/data/beta.i
  $ echo beta > beta

Test missing filelog entries:

  $ cp .hg/store/data/beta.i tmp
  $ echo blah >> beta
  $ hg ci -m '2 (corrupt)'
  $ mv tmp .hg/store/data/beta.i

Expected to fail:

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
   beta@1: manifest refers to unknown revision dddc47b3ba30
  2 files, 2 changesets, 2 total revisions
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

  $ hg push
  pushing to $TESTTMP/test (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  transaction abort!
  rollback completed
  abort: missing file data for beta:dddc47b3ba30e54484720ce0f4f768a0f4b6efb9 - run hg verify
  [255]

  $ cd ..
