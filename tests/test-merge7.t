initial
  $ hg init test-a
  $ cd test-a
  $ cat >test.txt <<"EOF"
  > 1
  > 2
  > 3
  > EOF
  $ hg add test.txt
  $ hg commit -m "Initial" -d "1000000 0"

clone
  $ cd ..
  $ hg clone test-a test-b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

change test-a
  $ cd test-a
  $ cat >test.txt <<"EOF"
  > one
  > two
  > three
  > EOF
  $ hg commit -m "Numbers as words" -d "1000000 0"

change test-b
  $ cd ../test-b
  $ cat >test.txt <<"EOF"
  > 1
  > 2.5
  > 3
  > EOF
  $ hg commit -m "2 -> 2.5" -d "1000000 0"

now pull and merge from test-a
  $ hg pull ../test-a
  pulling from ../test-a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  merging test.txt
  warning: conflicts during merge.
  merging test.txt failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C' to abandon
resolve conflict
  $ cat >test.txt <<"EOF"
  > one
  > two-point-five
  > three
  > EOF
  $ rm -f *.orig
  $ hg resolve -m test.txt
  $ hg commit -m "Merge 1" -d "1000000 0"

change test-a again
  $ cd ../test-a
  $ cat >test.txt <<"EOF"
  > one
  > two-point-one
  > three
  > EOF
  $ hg commit -m "two -> two-point-one" -d "1000000 0"

pull and merge from test-a again
  $ cd ../test-b
  $ hg pull ../test-a
  pulling from ../test-a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge --debug
    searching for copies back to rev 1
  resolving manifests
   overwrite None partial False
   ancestor faaea63e63a9 local 451c744aabcc+ remote a070d41e8360
   test.txt: versions differ -> m
  preserving test.txt for resolve of test.txt
  updating: test.txt 1/1 files (100.00%)
  picked tool 'internal:merge' for test.txt (binary False symlink False)
  merging test.txt
  my test.txt@451c744aabcc+ other test.txt@a070d41e8360 ancestor test.txt@faaea63e63a9
  warning: conflicts during merge.
  merging test.txt failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C' to abandon

  $ cat test.txt
  one
  <<<<<<< local
  two-point-five
  =======
  two-point-one
  >>>>>>> other
  three

  $ hg debugindex .hg/store/data/test.txt.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       7      0       0 01365c4cca56 000000000000 000000000000
       1         7       9      1       1 7b013192566a 01365c4cca56 000000000000
       2        16      15      2       2 8fe46a3eb557 01365c4cca56 000000000000
       3        31      27      2       3 fc3148072371 7b013192566a 8fe46a3eb557
       4        58      25      4       4 d40249267ae3 8fe46a3eb557 000000000000

  $ hg log
  changeset:   4:a070d41e8360
  tag:         tip
  parent:      2:faaea63e63a9
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     two -> two-point-one
  
  changeset:   3:451c744aabcc
  parent:      1:e409be6afcc0
  parent:      2:faaea63e63a9
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Merge 1
  
  changeset:   2:faaea63e63a9
  parent:      0:095c92b91f1a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Numbers as words
  
  changeset:   1:e409be6afcc0
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     2 -> 2.5
  
  changeset:   0:095c92b91f1a
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     Initial
  
