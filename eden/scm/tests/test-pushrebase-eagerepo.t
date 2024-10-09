#modern-config-incompatible
#require no-eden
#inprocess-hg-incompatible

  $ setconfig devel.segmented-changelog-rev-compat=true
  $ configure mutation dummyssh
  $ setconfig push.edenapi=true

Set up server repository

  $ newserver server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ echo foo > a
  $ echo foo > b
  $ hg commit -Am 'initial'
  adding a
  adding b
  $ hg book master
  $ cd ..

Set up client repository

  $ hg clone --config 'extensions.remotenames=' ssh://user@dummy/server client -q

Test that pushing to a remotename preserves commit hash if no rebase happens

  $ cd client
  $ setconfig extensions.remotenames= extensions.pushrebase=
  $ hg up -q master
  $ echo x >> a && hg commit -qm 'add a'
  $ hg commit --amend -qm 'changed message'
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg push --to master -q
tofix: push should create a new node after pushrebase
  $ hg show
  commit:      ea98a8f95390
  bookmark:    default/master
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
  @  ea98a8f95390 changed message default/master
  │
  o  2bb9d20e471c initial

tofix: the master bookmark should point to the latest commit
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' ssh://user@dummy/server client1 -q
  $ cd client1
  $ hg log -G -r 'all()' -T '{node|short} {desc} {remotebookmarks} {bookmarks}'
  @  ea98a8f95390 changed message
  │
  o  2bb9d20e471c initial default/master
