This test tries to exercise the ssh functionality with a dummy script

creating 'remote' repo

  $ hg init remote
  $ cd remote
  $ hg unbundle "$TESTDIR/bundles/remote.hg"
  adding changesets
  adding manifests
  adding file changes
  added 9 changesets with 7 changes to 4 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

clone remote via stream

  $ for i in 0 1 2 3 4 5 6 7 8; do
  >    hg clone -e dummyssh --uncompressed -r "$i" ssh://user@dummy/remote test-"$i"
  >    if cd test-"$i"; then
  >       hg verify
  >       cd ..
  >    fi
  > done
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 1 changesets, 1 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 4 changesets, 4 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 4 changesets, 5 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 3 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 5 changesets, 6 total revisions
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 2 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 5 total revisions
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
  $ cd ..
  $ cd test-1
  $ hg pull -e dummyssh -r 4 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 2 total revisions
  $ hg pull -e dummyssh ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 5 changes to 4 files
  (run 'hg update' to get a working copy)
  $ cd ..
  $ cd test-2
  $ hg pull -e dummyssh -r 5 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 5 changesets, 3 total revisions
  $ hg pull -e dummyssh ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 4 files
  (run 'hg update' to get a working copy)
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 9 changesets, 7 total revisions

  $ cd ..
