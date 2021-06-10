  $ disable treemanifest
Load commonly used test logic
  $ . "$TESTDIR/hggit/testutil"

  $ enable remotenames
  $ hg init test
  $ cd test
  $ cat >>afile <<EOF
  > 0
  > EOF
  $ hg add afile
  $ hg commit -m "0.0"
  $ cat >>afile <<EOF
  > 1
  > EOF
  $ hg commit -m "0.1"
  $ cat >>afile <<EOF
  > 2
  > EOF
  $ hg commit -m "0.2"
  $ cat >>afile <<EOF
  > 3
  > EOF
  $ hg commit -m "0.3"
  $ hg update -C 'desc(0.0)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat >>afile <<EOF
  > 1
  > EOF
  $ hg commit -m "1.1"
  $ cat >>afile <<EOF
  > 2
  > EOF
  $ hg commit -m "1.2"
  $ cat >fred <<EOF
  > a line
  > EOF
  $ cat >>afile <<EOF
  > 3
  > EOF
  $ hg add fred
  $ hg commit -m "1.3"
  $ hg mv afile adifferentfile
  $ hg commit -m "1.3m"
  $ hg update -C 'desc(0.3)'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg mv afile anotherfile
  $ hg commit -m "0.3m"
  $ cd ..
  $ for i in 0 1 2 3 4 5 6 7 8; do
  >    mkdir test-"$i"
  >    hg --cwd test-"$i" init
  >    hg -R test push -r "$i" test-"$i" --to master --force --create
  >    cd test-"$i"
  >    hg verify
  >    cd ..
  > done
  pushing rev f9ee2f85a263 to destination test-0 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev 34c2bf6b0626 to destination test-1 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev e38ba6f5b7e0 to destination test-2 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev eebf5a27f8ca to destination test-3 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev 095197eb4973 to destination test-4 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev 1bb50a9436a7 to destination test-5 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev 7373c1169842 to destination test-6 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 2 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev a6a34bfa0076 to destination test-7 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 6 changes to 3 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  pushing rev aa35859c02ea to destination test-8 bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 2 files
  exporting bookmark master
  warning: verify does not actually check anything in this repo
  $ cd test-8
  $ hg pull ../test-7
  pulling from ../test-7
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 2 changes to 3 files
  $ hg verify
  warning: verify does not actually check anything in this repo
