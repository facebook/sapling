#require no-eden

  $ configure mutation

  $ function log() {
  >   hg log -G -r 'all()' -T '{node|short} {desc} {remotebookmarks} {bookmarks}'
  > }

#testcases slapi 

#if wireproto
  $ setconfig push.edenapi=false
#else
  $ setconfig push.edenapi=true
#endif

Set up server repository

#if wireproto
  $ rm $TESTTMP/.eagerepo
#endif
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

Test fast forward push

  $ newclientrepo client test:server
  $ hg up -q master
  $ echo x >> a && hg commit -qm 'add a'
  $ hg commit --amend -qm 'changed message'
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg push --to master -q
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
  $ log
  @  ea98a8f95390 changed message remote/master
  │
  o  2bb9d20e471c initial

the master bookmark should point to the latest commit
  $ newclientrepo client2 test:server
  $ log
  @  ea98a8f95390 changed message remote/master
  │
  o  2bb9d20e471c initial

test pushrebase conflicts
  $ hg go -q 2bb9d20e471c
  $ echo y >> a && hg commit -qm "update a"
#if slapi
  $ hg push --to master -q
  abort: Server error: Conflicts while pushrebasing: [(RepoPathBuf("a"), RepoPathBuf("a"))]
  [255]
#endif

Test pushrebase a diff stack
  $ newclientrepo client3 test:server
  $ hg go -q 2bb9d20e471c
  $ echo 1 >> c && hg ci -qAm "add c"
  $ echo 2 >> c && hg ci -qm "update c"
  $ log
  @  adb87132efa9 update c
  │
  o  f46b94d12452 add c
  │
  │ o  ea98a8f95390 changed message remote/master
  ├─╯
  o  2bb9d20e471c initial
  $ hg push --to master -q
  $ log
  @  9237cb52bef6 update c remote/master
  │
  o  d7f85d6d9fc3 add c
  │
  o  ea98a8f95390 changed message
  │
  o  2bb9d20e471c initial
