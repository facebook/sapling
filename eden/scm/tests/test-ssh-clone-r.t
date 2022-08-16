#chg-compatible
#debugruntest-compatible
  $ setconfig workingcopy.ruststatus=False
  $ setconfig experimental.allowfilepeer=True

  $ disable treemanifest
  $ configure dummyssh
This test tries to exercise the ssh functionality with a dummy script

creating 'remote' repo

  $ hg init remote
  $ cd remote
  $ hg unbundle "$TESTDIR/bundles/remote.hg"
  adding changesets
  adding manifests
  adding file changes
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..

clone remote via stream

  $ for i in 0 1 2 3 4 5 6 7 8; do
  >    hg clone --stream -r "$i" ssh://user@dummy/remote test-"$i"
  >    if cd test-"$i"; then
  >       hg verify
  >       cd ..
  >    fi
  > done
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: verify does not actually check anything in this repo
  $ cd test-8
  $ hg pull ../test-7
  pulling from ../test-7
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ cd ..
  $ cd test-1
  $ hg pull -r 4 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ hg pull ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ cd ..
  $ cd test-2
  $ hg pull -r 5 ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo
  $ hg pull ssh://user@dummy/remote
  pulling from ssh://user@dummy/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg verify
  warning: verify does not actually check anything in this repo

  $ cd ..
