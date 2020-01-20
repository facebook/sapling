  $ disable treemanifest
  $ configure mutation dummyssh

Set up server repository

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > pushrebase=
  > remotenames = !
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
  $ cp -R server server1
  $ hg clone --config 'extensions.remotenames=' ssh://user@dummy/server1 client1 -q

Test that pushing to a remotename preserves commit hash if no rebase happens

  $ cd client1
  $ setconfig extensions.remotenames= extensions.pushrebase=
  $ hg up -q master
  $ echo x >> a && hg commit -qm 'add a'
  $ hg commit --amend -qm 'changed message'
  $ hg log -r . -T '{node}\n'
  ea98a8f9539083f60b81315106c94227e8814d17
  $ hg push --to master
  pushing rev ea98a8f95390 to destination ssh://user@dummy/server1 bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     ea98a8f95390  changed message
  remote: 1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  updating bookmark master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{node}\n'
  a59527fd0ae5acd6fe09597193f5eb3e01113f22
  $ hg log -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  changed message default/master
  |
  o  initial
  
  $ cd ..

Test that pushing to a remotename gets rebased

  $ cd server
  $ hg up -q master
  $ echo x >> a && hg commit -m "master's commit"
  $ cd ../client
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > remotenames =
  > pushrebase=
  > [remotenames]
  > allownonfastforward=True
  > EOF
  $ echo x >> b && hg commit -m "client's commit"
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  1 "client's commit"
  |
  o  0 "initial" default/master
  

 (disable remotenames.racy-pull-on-push so we can check pushrebase's fallback behavior on updating remotenames)
  $ hg push --to master --config remotenames.racy-pull-on-push=0
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     5c3cfb78df2f  client's commit
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  updating bookmark master
  moving remote bookmark 'default/master' to 98d6f1036c3b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  3 "client's commit" default/master
  |
  o  2 "master's commit"
  |
  o  0 "initial"
  

  $ cd ../server
  $ hg log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "client's commit" master
  |
  @  1 "master's commit"
  |
  o  0 "initial"
  
Test pushing a new bookmark
  $ cd ..
  $ hg -R client push --to newbook
  pushing rev 98d6f1036c3b to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]

  $ hg -R client push --to newbook --create
  pushing rev 98d6f1036c3b to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  no changes found
  exporting bookmark newbook
  [1]
  $ hg -R server book
   * master                    2:98d6f1036c3b
     newbook                   2:98d6f1036c3b
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "client's commit" master newbook
  |
  @  1 "master's commit"
  |
  o  0 "initial"
  
  $ hg log -R client -G -r 'all()' -T '{desc} {remotebookmarks}'
  @  client's commit default/master default/newbook
  |
  o  master's commit
  |
  o  initial
  
Test doing a non-fastforward bookmark move

  $ hg -R client push --to newbook -r master -f
  pushing rev 98d6f1036c3b to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  no changes found
  updating bookmark newbook
  [1]
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "client's commit" master newbook
  |
  @  1 "master's commit"
  |
  o  0 "initial"
  
  $ hg log -R client -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  client's commit default/master default/newbook
  |
  o  master's commit
  |
  o  initial
  

Test a push that comes with out-of-date bookmark discovery

  $ hg -R server debugstrip -q 0
  $ hg -R client debugstrip -q 0

  $ hg bookmarks --cwd server -d master newbook

  $ echo a >> server/a
  $ hg -R server commit -qAm 'aa'
  $ hg -R server bookmark bm -i
  $ echo b >> server/b
  $ hg -R server commit -qAm 'bb'
  $ hg log -R client -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'

  $ cat >> $TESTTMP/move.py <<EOF
  > def movebookmark(ui, repo, **kwargs):
  >     import traceback
  >     if [f for f in traceback.extract_stack(limit=10)[:-1] if f[2] == "movebookmark"]:
  >         return
  >     import edenscm.mercurial.lock as lockmod
  >     tr = None
  >     try:
  >         lock = repo.lock()
  >         tr = repo.transaction("pretxnopen.movebook")
  >         changes = [('bm', repo[1].node())]
  >         repo._bookmarks.applychanges(repo, tr, changes)
  >         tr.close()
  >     finally:
  >         if tr:
  >             tr.release()
  >         lockmod.release(lock)
  >     print "moved bookmark to rev 1"
  > EOF
  $ cat >> server/.hg/hgrc <<EOF
  > [hooks]
  > pretxnopen.movebook = python:$TESTTMP/move.py:movebookmark
  > EOF
  $ hg -R client pull -q -r 0
  $ hg -R client update -q 0
  $ echo c >> client/c
  $ hg -R client commit -qAm 'cc'
  $ hg -R client log -G -T '{rev} "{desc}" {bookmarks}'
  @  1 "cc"
  |
  o  0 "aa"
  
  $ hg -R client push --to bm
  pushing rev 5db65b93a12b to destination ssh://user@dummy/server bookmark bm
  searching for changes
  remote: moved bookmark to rev 1
  remote: pushing 1 changeset:
  remote:     5db65b93a12b  cc
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  updating bookmark bm
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "cc" bm
  |
  @  1 "bb"
  |
  o  0 "aa"
  
  $ hg -R client log -G -T '{rev} "{desc}" {bookmarks} {remotenames}'
  @  3 "cc"  default/bm
  |
  o  2 "bb"
  |
  o  0 "aa"
  

Test that we still don't allow non-ff bm changes

  $ echo d > client/d
  $ hg -R client commit -qAm "dd"
  $ hg -R client log -G -T '{rev} "{desc}" {bookmarks}'
  @  4 "dd"
  |
  o  3 "cc"
  |
  o  2 "bb"
  |
  o  0 "aa"
  

  $ hg -R client push --to bm
  pushing rev 2f9755549086 to destination ssh://user@dummy/server bookmark bm
  searching for changes
  remote: moved bookmark to rev 1
  remote: pushing 1 changeset:
  remote:     2f9755549086  dd
  remote: 1 new changeset from the server will be downloaded
  remote: transaction abort!
  remote: rollback completed
  abort: updating bookmark bm failed!
  [255]

Test force pushes
  $ hg init forcepushserver
  $ cd forcepushserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames = !
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book master
  $ cd ..

  $ hg clone -q --config 'extensions.remotenames=' ssh://user@dummy/forcepushserver forcepushclient
  $ cd forcepushserver
  $ echo a >> a && hg commit -Aqm aa

  $ cd ../forcepushclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames =
  > [remotenames]
  > allownonfastforward=True
  > EOF
  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a && hg commit -Aqm b
  $ hg push -f --to master
  pushing rev 1846eede8b68 to destination * (glob)
  searching for changes
  remote: pushing 1 changeset:
  remote:     1846eede8b68  b
  remote: 1 new changeset from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 1 files
  updating bookmark master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  b default/master
  |
  o  a
  
  $ hg pull
  pulling from * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg log -G -T '{rev} {desc} {remotebookmarks}'
  o  3 aa
  |
  | @  2 b default/master
  |/
  o  0 a
  
  $ cd ..

Test 'hg push' with a tracking bookmark
  $ hg init trackingserver
  $ cd trackingserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames = !
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book master
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' -q ssh://user@dummy/trackingserver trackingclient
  $ cd trackingclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames =
  > [remotenames]
  > allownonfastforward=True
  > EOF
  $ hg book feature -t default/master
  $ echo b > b && hg commit -Aqm b
  $ cd ../trackingserver
  $ echo c > c && hg commit -Aqm c
  $ cd ../trackingclient
  $ hg push
  pushing rev d2ae7f538514 to destination ssh://user@dummy/trackingserver bookmark master
  searching for changes
  remote: pushing 1 changeset:
  remote:     d2ae7f538514  b
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files
  updating bookmark master
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -T '{rev} {desc}' -G
  @  3 b
  |
  o  2 c
  |
  o  0 a
  
  $ cd ..

Test push --to to a repo without pushrebase on (i.e. the default remotenames behavior)
  $ hg init oldserver
  $ cd oldserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames =
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book serverfeature
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' -q ssh://user@dummy/oldserver newclient
  $ cd newclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames =
  > EOF
  $ hg book clientfeature -t default/serverfeature
  $ echo b > b && hg commit -Aqm b
  $ hg push --to serverfeature
  pushing rev d2ae7f538514 to destination ssh://user@dummy/oldserver bookmark serverfeature
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files
  updating bookmark serverfeature
  $ hg log -G -T '{shortest(node)} {bookmarks}'
  @  d2ae clientfeature
  |
  o  cb9a
  
  $ cd ../oldserver
  $ hg log -G -T '{shortest(node)} {bookmarks}'
  o  d2ae serverfeature
  |
  @  cb9a
  
Test push --to with remotenames but without pushrebase to a remote repository
that requires pushrebase.

  $ cd ..
  $ hg init pushrebaseserver
  $ cd pushrebaseserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames =
  > pushrebase=
  > [pushrebase]
  > blocknonpushrebase = True
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book serverfeature
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' -q ssh://user@dummy/pushrebaseserver remotenamesonlyclient
  $ cd remotenamesonlyclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=!
  > remotenames =
  > EOF
  $ hg book clientfeature -t default/serverfeature
  $ echo b > b && hg commit -Aqm b
  $ hg push --to serverfeature
  pushing rev d2ae7f538514 to destination ssh://user@dummy/pushrebaseserver bookmark serverfeature
  searching for changes
  remote: error: prechangegroup.blocknonpushrebase hook failed: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  remote: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  abort: push failed on remote
  [255]

