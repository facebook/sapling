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

  $ cd test-clone
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
   beta@1: dddc47b3ba30 in manifests not found
  2 files, 2 changesets, 2 total revisions
  1 integrity errors encountered!
  (first damaged changeset appears to be 1)
  [1]

Expected to fail:

  $ hg push
  pushing to $TESTTMP/test
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  transaction abort!
  rollback completed
  abort: missing file data for beta:dddc47b3ba30e54484720ce0f4f768a0f4b6efb9 - run hg verify
  [255]

