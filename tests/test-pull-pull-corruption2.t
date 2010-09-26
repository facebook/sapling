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

create a third repo to pull both other repos into it

  $ hg init version2
  $ hg -R version2 pull source1 &
  $ sleep 1
  pulling from source1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 10 changesets with 10 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg clone --pull -U version2 corrupted
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 10 changesets with 10 changes to 1 files
  $ wait
  $ hg -R corrupted verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 10 changesets, 10 total revisions
  $ hg -R version2 verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 10 changesets, 10 total revisions
