#chg-compatible
#debugruntest-compatible
  $ configure modernclient

initial
  $ newclientrepo test-a
  $ cat >test.txt <<"EOF"
  > 1
  > 2
  > 3
  > EOF
  $ hg add test.txt
  $ hg commit -m "Initial"
  $ hg push -q --to book --create

clone
  $ newclientrepo test-b test:test-a_server book

change test-a
  $ cd ../test-a
  $ cat >test.txt <<"EOF"
  > one
  > two
  > three
  > EOF
  $ hg commit -m "Numbers as words"
  $ hg push -q --to book

change test-b
  $ cd ../test-b
  $ cat >test.txt <<"EOF"
  > 1
  > 2.5
  > 3
  > EOF
  $ hg commit -m "2 -> 2.5"

now pull and merge from test-a
  $ hg pull test:test-a_server
  pulling from test:test-a_server
  searching for changes
  $ hg merge 'desc("Numbers as words")'
  merging test.txt
  warning: 1 conflicts while merging test.txt! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
resolve conflict
  $ cat >test.txt <<"EOF"
  > one
  > two-point-five
  > three
  > EOF
  $ rm -f *.orig
  $ hg resolve -m test.txt
  (no more unresolved files)
  $ hg commit -m "Merge 1"

change test-a again
  $ cd ../test-a
  $ cat >test.txt <<"EOF"
  > one
  > two-point-one
  > three
  > EOF
  $ hg commit -m "two -> two-point-one"
  $ hg push -q --to book

pull and merge from test-a again
  $ cd ../test-b
  $ hg pull test:test-a_server
  pulling from test:test-a_server
  searching for changes
  $ hg merge --debug
    searching for copies back to d1e159716d41
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 96b70246a118, local: 50c3a7e29886+, remote: 40d11a4173a8
   preserving test.txt for resolve of test.txt
   test.txt: versions differ -> m (premerge)
  picktool() hgmerge internal:merge
  picked tool ':merge' for test.txt (binary False symlink False changedelete False)
  merging test.txt
  my test.txt@50c3a7e29886+ other test.txt@40d11a4173a8 ancestor test.txt@96b70246a118
   test.txt: versions differ -> m (merge)
  picktool() hgmerge internal:merge
  picked tool ':merge' for test.txt (binary False symlink False changedelete False)
  my test.txt@50c3a7e29886+ other test.txt@40d11a4173a8 ancestor test.txt@96b70246a118
  warning: 1 conflicts while merging test.txt! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]

  $ cat test.txt
  one
  <<<<<<< working copy: 50c3a7e29886 - test: Merge 1
  two-point-five
  =======
  two-point-one
  >>>>>>> merge rev:    40d11a4173a8 - test: two -> two-point-one
  three

  $ hg log
  commit:      40d11a4173a8
  bookmark:    remote/book
  hoistedname: book
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     two -> two-point-one
  
  commit:      50c3a7e29886
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Merge 1
  
  commit:      96b70246a118
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Numbers as words
  
  commit:      d1e159716d41
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2.5
  
  commit:      b1832b9d912a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Initial
  

  $ cd ..
