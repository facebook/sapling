#require no-eden
#inprocess-hg-incompatible

  $ configure mutation
  $ setconfig push.edenapi=true

Set up server repository

  $ newserver server
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book master
  $ cd ..

Test fast forward push

  $ newclientrepo client test:server
  $ hg up -q master
  $ echo x >> a && hg commit -qm 'add a'
  $ hg commit --amend -qm 'changed message'
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg push --to master -q
tofix: push should create a new node after pushrebase
  $ hg show
  commit:      ea98a8f95390
  bookmark:    remote/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  description:
  changed message
  
  
  diff -r 2bb9d20e471c -r ea98a8f95390 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   foo
  +x
  $ hg log -G -r 'all()' -T '{node|short} {desc} {remotebookmarks} {bookmarks}'
  @  ea98a8f95390 changed message remote/master
  │
  o  2bb9d20e471c initial

the master bookmark should point to the latest commit
  $ newclientrepo client2 test:server
  $ hg log -G -r 'all()' -T '{node|short} {desc} {remotebookmarks} {bookmarks}'
  @  ea98a8f95390 changed message remote/master
  │
  o  2bb9d20e471c initial
