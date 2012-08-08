Preparing the subrepository 'sub2'

  $ hg init sub2
  $ echo sub2 > sub2/sub2
  $ hg add -R sub2
  adding sub2/sub2 (glob)
  $ hg commit -R sub2 -m "sub2 import"

Preparing the 'sub1' repo which depends on the subrepo 'sub2'

  $ hg init sub1
  $ echo sub1 > sub1/sub1
  $ echo "sub2 = ../sub2" > sub1/.hgsub
  $ hg clone sub2 sub1/sub2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg add -R sub1
  adding sub1/.hgsub (glob)
  adding sub1/sub1 (glob)
  $ hg commit -R sub1 -m "sub1 import"

Preparing the 'main' repo which depends on the subrepo 'sub1'

  $ hg init main
  $ echo main > main/main
  $ echo "sub1 = ../sub1" > main/.hgsub
  $ hg clone sub1 main/sub1
  updating to branch default
  cloning subrepo sub2 from $TESTTMP/sub2
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg add -R main
  adding main/.hgsub (glob)
  adding main/main (glob)
  $ hg commit -R main -m "main import"

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
  cloning subrepo sub1 from $TESTTMP/sub1
  cloning subrepo sub1/sub2 from $TESTTMP/sub2 (glob)
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
  $ hg commit --subrepos -m "deep nested modif should trigger a commit" -R cloned
  committing subrepository sub1
  committing subrepository sub1/sub2 (glob)

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

Check that deep archiving works

  $ cd cloned
  $ echo 'test' > sub1/sub2/test.txt
  $ hg --config extensions.largefiles=! add sub1/sub2/test.txt
  $ mkdir sub1/sub2/folder
  $ echo 'subfolder' > sub1/sub2/folder/test.txt
  $ hg --config extensions.largefiles=! add sub1/sub2/folder/test.txt
  $ hg ci -Sm "add test.txt"
  committing subrepository sub1
  committing subrepository sub1/sub2 (glob)
  $ hg --config extensions.largefiles=! archive -S ../archive_all
  $ find ../archive_all | sort
  ../archive_all
  ../archive_all/.hg_archival.txt
  ../archive_all/.hgsub
  ../archive_all/.hgsubstate
  ../archive_all/main
  ../archive_all/sub1
  ../archive_all/sub1/.hgsub
  ../archive_all/sub1/.hgsubstate
  ../archive_all/sub1/sub1
  ../archive_all/sub1/sub2
  ../archive_all/sub1/sub2/folder
  ../archive_all/sub1/sub2/folder/test.txt
  ../archive_all/sub1/sub2/sub2
  ../archive_all/sub1/sub2/test.txt

Check that archive -X works in deep subrepos

  $ hg --config extensions.largefiles=! archive -S -X '**test*' ../archive_exclude
  $ find ../archive_exclude | sort
  ../archive_exclude
  ../archive_exclude/.hg_archival.txt
  ../archive_exclude/.hgsub
  ../archive_exclude/.hgsubstate
  ../archive_exclude/main
  ../archive_exclude/sub1
  ../archive_exclude/sub1/.hgsub
  ../archive_exclude/sub1/.hgsubstate
  ../archive_exclude/sub1/sub1
  ../archive_exclude/sub1/sub2
  ../archive_exclude/sub1/sub2/sub2

  $ hg --config extensions.largefiles=! archive -S -I '**test*' ../archive_include
  $ find ../archive_include | sort
  ../archive_include
  ../archive_include/sub1
  ../archive_include/sub1/sub2
  ../archive_include/sub1/sub2/folder
  ../archive_include/sub1/sub2/folder/test.txt
  ../archive_include/sub1/sub2/test.txt

Check that deep archive works with largefiles (which overrides hgsubrepo impl)
This also tests the repo.ui regression in 43fb170a23bd, and that lf subrepo
subrepos are archived properly.
Note that add --large through a subrepo currently adds the file as a normal file

  $ echo "large" > sub1/sub2/large.bin
  $ hg --config extensions.largefiles= add --large -R sub1/sub2 sub1/sub2/large.bin
  $ echo "large" > large.bin
  $ hg --config extensions.largefiles= add --large large.bin
  $ hg --config extensions.largefiles= ci -S -m "add large files"
  committing subrepository sub1
  committing subrepository sub1/sub2 (glob)

  $ hg --config extensions.largefiles= archive -S ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/.hg_archival.txt
  ../archive_lf/.hgsub
  ../archive_lf/.hgsubstate
  ../archive_lf/large.bin
  ../archive_lf/main
  ../archive_lf/sub1
  ../archive_lf/sub1/.hgsub
  ../archive_lf/sub1/.hgsubstate
  ../archive_lf/sub1/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/folder
  ../archive_lf/sub1/sub2/folder/test.txt
  ../archive_lf/sub1/sub2/large.bin
  ../archive_lf/sub1/sub2/sub2
  ../archive_lf/sub1/sub2/test.txt
  $ rm -rf ../archive_lf

Exclude large files from main and sub-sub repo

  $ hg --config extensions.largefiles= archive -S -X '**.bin' ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/.hg_archival.txt
  ../archive_lf/.hgsub
  ../archive_lf/.hgsubstate
  ../archive_lf/main
  ../archive_lf/sub1
  ../archive_lf/sub1/.hgsub
  ../archive_lf/sub1/.hgsubstate
  ../archive_lf/sub1/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/folder
  ../archive_lf/sub1/sub2/folder/test.txt
  ../archive_lf/sub1/sub2/sub2
  ../archive_lf/sub1/sub2/test.txt
  $ rm -rf ../archive_lf

Exclude normal files from main and sub-sub repo

  $ hg --config extensions.largefiles= archive -S -X '**.txt' ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/.hgsub
  ../archive_lf/.hgsubstate
  ../archive_lf/large.bin
  ../archive_lf/main
  ../archive_lf/sub1
  ../archive_lf/sub1/.hgsub
  ../archive_lf/sub1/.hgsubstate
  ../archive_lf/sub1/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/large.bin
  ../archive_lf/sub1/sub2/sub2
  $ rm -rf ../archive_lf

Include normal files from within a largefiles subrepo

  $ hg --config extensions.largefiles= archive -S -I '**.txt' ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/.hg_archival.txt
  ../archive_lf/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/folder
  ../archive_lf/sub1/sub2/folder/test.txt
  ../archive_lf/sub1/sub2/test.txt
  $ rm -rf ../archive_lf

Include large files from within a largefiles subrepo

  $ hg --config extensions.largefiles= archive -S -I '**.bin' ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/large.bin
  ../archive_lf/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/large.bin
  $ rm -rf ../archive_lf

Find an exact largefile match in a largefiles subrepo

  $ hg --config extensions.largefiles= archive -S -I 'sub1/sub2/large.bin' ../archive_lf
  $ find ../archive_lf | sort
  ../archive_lf
  ../archive_lf/sub1
  ../archive_lf/sub1/sub2
  ../archive_lf/sub1/sub2/large.bin
  $ rm -rf ../archive_lf

Find an exact match to a standin (should archive nothing)
  $ hg --config extensions.largefiles= archive -S -I 'sub/sub2/.hglf/large.bin' ../archive_lf
  $ find ../archive_lf 2> /dev/null | sort

  $ cd ..
