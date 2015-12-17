  $ hg init test
  $ cd test
  $ hg unbundle "$TESTDIR/bundles/remote.hg"
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

  $ for i in 0 1 2 3 4 5 6 7 8; do
  >    mkdir test-"$i"
  >    hg --cwd test-"$i" init
  >    hg -R test bundle -r "$i" test-"$i".hg test-"$i"
  >    cd test-"$i"
  >    hg unbundle ../test-"$i".hg
  >    hg verify
  >    hg tip -q
  >    cd ..
  > done
  searching for changes
  1 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  0:bfaf4b5cbf01
  searching for changes
  2 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions
  1:21f32785131f
  searching for changes
  3 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  2:4ce51a113780
  searching for changes
  4 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 4 changesets, 4 total revisions
  3:93ee6ab32777
  searching for changes
  2 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions
  1:c70afb1ee985
  searching for changes
  3 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  2:f03ae5a9b979
  searching for changes
  4 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 2 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 4 changesets, 5 total revisions
  3:095cb14b1b4d
  searching for changes
  5 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 3 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 5 changesets, 6 total revisions
  4:faa2e4234c7a
  searching for changes
  5 changesets found
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 2 files
  (run 'hg update' to get a working copy)
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 5 total revisions
  4:916f1afdef90
  $ cd test-8
  $ hg pull ../test-7
  pulling from ../test-7
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 3 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 9 changesets, 7 total revisions
  $ hg rollback
  repository tip rolled back to revision 4 (undo pull)
  $ cd ..

should fail

  $ hg -R test bundle --base 2 -r tip test-bundle-branch1.hg test-3
  abort: --base is incompatible with specifying a destination
  [255]
  $ hg -R test bundle -a -r tip test-bundle-branch1.hg test-3
  abort: --all is incompatible with specifying a destination
  [255]
  $ hg -R test bundle -r tip test-bundle-branch1.hg
  abort: repository default-push not found!
  [255]

  $ hg -R test bundle --base 2 -r tip test-bundle-branch1.hg
  2 changesets found
  $ hg -R test bundle --base 2 -r 7 test-bundle-branch2.hg
  4 changesets found
  $ hg -R test bundle --base 2 test-bundle-all.hg
  6 changesets found
  $ hg -R test bundle --base 2 --all test-bundle-all-2.hg
  ignoring --base because --all was specified
  9 changesets found
  $ hg -R test bundle --base 3 -r tip test-bundle-should-fail.hg
  1 changesets found

empty bundle

  $ hg -R test bundle --base 7 --base 8 test-bundle-empty.hg
  no changes found
  [1]

issue76 msg2163

  $ hg -R test bundle --base 3 -r 3 -r 3 test-bundle-cset-3.hg
  no changes found
  [1]

Issue1910: 'hg bundle --base $head' does not exclude $head from
result

  $ hg -R test bundle --base 7 test-bundle-cset-7.hg
  4 changesets found

  $ hg clone test-2 test-9
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test-9

revision 2

  $ hg tip -q
  2:4ce51a113780
  $ hg unbundle ../test-bundle-should-fail.hg
  adding changesets
  transaction abort!
  rollback completed
  abort: 00changelog.i@93ee6ab32777: unknown parent!
  [255]

revision 2

  $ hg tip -q
  2:4ce51a113780
  $ hg unbundle ../test-bundle-all.hg
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 4 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

revision 8

  $ hg tip -q
  8:916f1afdef90
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 9 changesets, 7 total revisions
  $ hg rollback
  repository tip rolled back to revision 2 (undo unbundle)

revision 2

  $ hg tip -q
  2:4ce51a113780
  $ hg unbundle ../test-bundle-branch1.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

revision 4

  $ hg tip -q
  4:916f1afdef90
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 5 total revisions
  $ hg rollback
  repository tip rolled back to revision 2 (undo unbundle)
  $ hg unbundle ../test-bundle-branch2.hg
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 3 changes to 3 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

revision 6

  $ hg tip -q
  6:faa2e4234c7a
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 7 changesets, 6 total revisions
  $ hg rollback
  repository tip rolled back to revision 2 (undo unbundle)
  $ hg unbundle ../test-bundle-cset-7.hg
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  (run 'hg update' to get a working copy)

revision 4

  $ hg tip -q
  4:916f1afdef90
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 5 total revisions

  $ cd ../test
  $ hg merge 7
  note: possible conflict - afile was renamed multiple times to:
   anotherfile
   adifferentfile
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ cd ..
  $ hg -R test bundle --base 2 test-bundle-head.hg
  7 changesets found
  $ hg clone test-2 test-10
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test-10
  $ hg unbundle ../test-bundle-head.hg
  adding changesets
  adding manifests
  adding file changes
  added 7 changesets with 4 changes to 4 files
  (run 'hg update' to get a working copy)

revision 9

  $ hg tip -q
  9:03fc0b0e347c
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 10 changesets, 7 total revisions

  $ cd ..
