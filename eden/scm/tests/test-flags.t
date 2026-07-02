
#require execbit no-eden

  $ configure modernclient

  $ umask 027

  $ newclientrepo test1
  $ touch a b
  $ sl add a b
  $ sl ci -m "added a b"
  $ sl push -r . -q --to book --create

  $ newclientrepo test3 test1_server book

  $ newclientrepo test2 test1_server book
  $ chmod u+x a
  $ chmod g+x a
  $ sl ci -m "chmod +x a"
  $ sl push -q -r . --to book2 --create

the changelog should mention file a:

  $ sl tip --template '{files}\n'
  a

  $ cd ../test1
  $ echo 123 >>a
  $ sl ci -m "a updated"
  $ sl push -q -r . --to book1 --create

  $ sl pull -B book2
  pulling from test:test1_server
  searching for changes
  $ sl heads
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
  
  $ sl log
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
  

  $ sl -v merge
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat a
  123
  $ [ -x a ]

  $ cd ../test3
  $ echo 123 >>b
  $ sl ci -m "b updated"

  $ sl pull test:test1_server -B book1 -B book2
  pulling from test:test1_server
  searching for changes
  $ sl heads
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
  
  $ sl log
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
  

  $ sl -v merge -r book2
  resolving manifests
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ f -m ../test1/a ../test2/a ../test3/a
  ../test1/a: mode=750
  ../test2/a: mode=750
  ../test3/a: mode=750
