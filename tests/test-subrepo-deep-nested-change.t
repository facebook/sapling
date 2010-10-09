Preparing the subrepository 'sub2'

  $ hg init sub2
  $ echo sub2 > sub2/sub2
  $ hg add -R sub2
  adding sub2/sub2
  $ hg commit -R sub2 -m "sub2 import"

Preparing the 'sub1' repo which depends on the subrepo 'sub2'

  $ hg init sub1
  $ echo sub1 > sub1/sub1
  $ echo "sub2 = ../sub2" > sub1/.hgsub
  $ hg clone sub2 sub1/sub2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg add -R sub1
  adding sub1/.hgsub
  adding sub1/sub1
  $ hg commit -R sub1 -m "sub1 import"
  committing subrepository sub2

Preparing the 'main' repo which depends on the subrepo 'sub1'

  $ hg init main
  $ echo main > main/main
  $ echo "sub1 = ../sub1" > main/.hgsub
  $ hg clone sub1 main/sub1
  updating to branch default
  pulling subrepo sub2 from $TESTTMP/sub2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg add -R main
  adding main/.hgsub
  adding main/main
  $ hg commit -R main -m "main import"
  committing subrepository sub1

Cleaning both repositories, just as a clone -U

  $ hg up -C -R sub2 null
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg up -C -R sub1 null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ hg up -C -R main null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ rm -rf main/sub1
  $ rm -rf sub1/sub2

Clone main

  $ hg clone main cloned
  updating to branch default
  pulling subrepo sub1 from $TESTTMP/sub1
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  pulling subrepo sub1/sub2 from $TESTTMP/sub2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

Checking cloned repo ids

  $ printf "cloned " ; hg id -R cloned
  cloned 7f491f53a367 tip
  $ printf "cloned/sub1 " ; hg id -R cloned/sub1
  cloned/sub1 fc3b4ce2696f tip
  $ printf "cloned/sub1/sub2 " ; hg id -R cloned/sub1/sub2
  cloned/sub1/sub2 c57a0840e3ba tip

debugsub output for main and sub1

  $ hg debugsub -R cloned
  path sub1
   source   ../sub1
   revision fc3b4ce2696f7741438c79207583768f2ce6b0dd
  $ hg debugsub -R cloned/sub1
  path sub2
   source   ../sub2
   revision c57a0840e3badd667ef3c3ef65471609acb2ba3c

Modifying deeply nested 'sub2'

  $ echo modified > cloned/sub1/sub2/sub2
  $ hg commit -m "deep nested modif should trigger a commit" -R cloned
  committing subrepository sub1
  committing subrepository sub1/sub2

Checking modified node ids

  $ printf "cloned " ; hg id -R cloned
  cloned ffe6649062fe tip
  $ printf "cloned/sub1 " ; hg id -R cloned/sub1
  cloned/sub1 2ecb03bf44a9 tip
  $ printf "cloned/sub1/sub2 " ; hg id -R cloned/sub1/sub2
  cloned/sub1/sub2 53dd3430bcaf tip

debugsub output for main and sub1

  $ hg debugsub -R cloned
  path sub1
   source   ../sub1
   revision 2ecb03bf44a94e749e8669481dd9069526ce7cb9
  $ hg debugsub -R cloned/sub1
  path sub2
   source   ../sub2
   revision 53dd3430bcaf5ab4a7c48262bcad6d441f510487
