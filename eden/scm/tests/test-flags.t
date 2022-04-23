#chg-compatible

#require execbit

  $ configure modernclient

  $ umask 027

  $ newclientrepo test1
  $ touch a b
  $ hg add a b
  $ hg ci -m "added a b"
  $ hg push -r . -q --to book --create

  $ newclientrepo test3 test:test1_server book

  $ newclientrepo test2 test:test1_server book
  $ chmod +x a
  $ hg ci -m "chmod +x a"
  $ hg push -q -r . --to book2 --create

the changelog should mention file a:

  $ hg tip --template '{files}\n'
  a

  $ cd ../test1
  $ echo 123 >>a
  $ hg ci -m "a updated"
  $ hg push -q -r . --to book1 --create

  $ hg pull -B book2
  pulling from test:test1_server
  searching for changes
  $ hg heads
  commit:      7f4313b42a34
  bookmark:    remote/book2
  hoistedname: book2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod +x a
  
  commit:      c6ecefc45368
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a updated
  
  $ hg history
  commit:      7f4313b42a34
  bookmark:    remote/book2
  hoistedname: book2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod +x a
  
  commit:      c6ecefc45368
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a updated
  
  commit:      22a449e20da5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     added a b
  

  $ hg -v merge
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat a
  123
  $ [ -x a ]

  $ cd ../test3
  $ echo 123 >>b
  $ hg ci -m "b updated"

  $ hg pull test:test1_server -B book1 -B book2
  pulling from test:test1_server
  searching for changes
  $ hg heads
  commit:      c6ecefc45368
  bookmark:    remote/book1
  hoistedname: book1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a updated
  
  commit:      7f4313b42a34
  bookmark:    remote/book2
  hoistedname: book2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod +x a
  
  commit:      dc57ead75f79
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b updated
  
  $ hg history
  commit:      c6ecefc45368
  bookmark:    remote/book1
  hoistedname: book1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a updated
  
  commit:      7f4313b42a34
  bookmark:    remote/book2
  hoistedname: book2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod +x a
  
  commit:      dc57ead75f79
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b updated
  
  commit:      22a449e20da5
  bookmark:    remote/book
  hoistedname: book
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     added a b
  

  $ hg -v merge -r book2
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ f -m ../test1/a ../test2/a ../test3/a
  ../test1/a: mode=750
  ../test2/a: mode=750
  ../test3/a: mode=750
