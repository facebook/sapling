  $ hg init test
  $ cd test

  $ echo 0 >> afile
  $ hg add afile
  $ hg commit -m "0.0"

  $ echo 1 >> afile
  $ hg commit -m "0.1"

  $ echo 2 >> afile
  $ hg commit -m "0.2"

  $ echo 3 >> afile
  $ hg commit -m "0.3"

  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo 1 >> afile
  $ hg commit -m "1.1"
  created new head

  $ echo 2 >> afile
  $ hg commit -m "1.2"

  $ echo a line > fred
  $ echo 3 >> afile
  $ hg add fred
  $ hg commit -m "1.3"
  $ hg mv afile adifferentfile
  $ hg commit -m "1.3m"

  $ hg update -C 3
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved

  $ hg mv afile anotherfile
  $ hg commit -m "0.3m"

  $ hg debugindex -f 1 afile
     rev flag   offset   length     size  .....   link     p1     p2       nodeid (re)
       0 0000        0        3        2  .....      0     -1     -1 362fef284ce2 (re)
       1 0000        3        5        4  .....      1      0     -1 125144f7e028 (re)
       2 0000        8        7        6  .....      2      1     -1 4c982badb186 (re)
       3 0000       15        9        8  .....      3      2     -1 19b1fc555737 (re)

  $ hg debugindex adifferentfile
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      75  .....       7 2565f3199a74 000000000000 000000000000 (re)

  $ hg debugindex anotherfile
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      75  .....       8 2565f3199a74 000000000000 000000000000 (re)

  $ hg debugindex fred
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       8  .....       6 12ab3bcc5ea4 000000000000 000000000000 (re)

  $ hg debugindex --manifest
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      48  .....       0 43eadb1d2d06 000000000000 000000000000 (re)
       1        48      48  .....       1 8b89697eba2c 43eadb1d2d06 000000000000 (re)
       2        96      48  .....       2 626a32663c2f 8b89697eba2c 000000000000 (re)
       3       144      48  .....       3 f54c32f13478 626a32663c2f 000000000000 (re)
       4       192      ..  .....       6 de68e904d169 626a32663c2f 000000000000 (re)
       5       2..      ..  .....       7 09bb521d218d de68e904d169 000000000000 (re)
       6       3..      54  .....       8 1fde233dfb0f f54c32f13478 000000000000 (re)

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  4 files, 9 changesets, 7 total revisions

  $ cd ..

  $ for i in 0 1 2 3 4 5 6 7 8; do
  >   echo
  >   echo ---- hg clone -r "$i" test test-"$i"
  >   hg clone -r "$i" test test-"$i"
  >   cd test-"$i"
  >   hg verify
  >   cd ..
  > done
  
  ---- hg clone -r 0 test test-0
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
  
  ---- hg clone -r 1 test test-1
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
  
  ---- hg clone -r 2 test test-2
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
  
  ---- hg clone -r 3 test test-3
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
  
  ---- hg clone -r 4 test test-4
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
  
  ---- hg clone -r 5 test test-5
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
  
  ---- hg clone -r 6 test test-6
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
  
  ---- hg clone -r 7 test test-7
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
  
  ---- hg clone -r 8 test test-8
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

  $ hg clone test test-9
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd test-9
  $ hg branch foobar
  marked working directory as branch foobar
  (branches are permanent and global, did you want a bookmark?)
  $ echo file2 >> file2
  $ hg add file2
  $ hg commit -m "changeset9"
  $ echo file3 >> file3
  $ hg add file3
  $ hg commit -m "changeset10"
  $ cd ..
  $ hg clone -r 9 -u foobar test-9 test-10
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 6 changes to 3 files
  updating to branch foobar
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved


