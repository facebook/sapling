#debugruntest-compatible
#inprocess-hg-incompatible
  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ setconfig experimental.allowfilepeer=True
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
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     ea98a8f95390  changed message
  remote: 1 new changeset from the server will be downloaded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -r . -T '{node}\n'
  a59527fd0ae5acd6fe09597193f5eb3e01113f22
  $ hg log -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  changed message default/master
  │
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
  $ hg log -G -T '"{desc}" {remotebookmarks}'
  @  "client's commit"
  │
  o  "initial" default/master
  

 (disable remotenames.racy-pull-on-push so we can check pushrebase's fallback behavior on updating remotenames)
  $ hg push --to master --config remotenames.racy-pull-on-push=0
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     5c3cfb78df2f  client's commit
  remote: 2 new changesets from the server will be downloaded
  moving remote bookmark 'default/master' to 98d6f1036c3b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log -G -T '"{desc}" {remotebookmarks}'
  @  "client's commit" default/master
  │
  o  "master's commit"
  │
  o  "initial"
  

  $ cd ../server
  $ hg log -G -T '"{desc}" {bookmarks}'
  o  "client's commit" master
  │
  @  "master's commit"
  │
  o  "initial"
  
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
  $ hg -R server book
   * master                    98d6f1036c3b
     newbook                   98d6f1036c3b
  $ hg -R server log -G -T '"{desc}" {bookmarks}'
  o  "client's commit" master newbook
  │
  @  "master's commit"
  │
  o  "initial"
  
  $ hg log -R client -G -r 'all()' -T '{desc} {remotebookmarks}'
  @  client's commit default/master default/newbook
  │
  o  master's commit
  │
  o  initial
  
Test doing a non-fastforward bookmark move

  $ hg -R client push --to newbook -r master -f
  pushing rev 98d6f1036c3b to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  no changes found
  updating bookmark newbook
  $ hg -R server log -G -T '"{desc}" {bookmarks}'
  o  "client's commit" master newbook
  │
  @  "master's commit"
  │
  o  "initial"
  
  $ hg log -R client -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  client's commit default/master default/newbook
  │
  o  master's commit
  │
  o  initial
  

Test a push that comes with out-of-date bookmark discovery

  $ hg -R server debugstrip -q 'desc(initial)'
  $ hg -R client debugstrip -q 'desc(initial)'

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
  >     ui.status("moved bookmark to rev 1\n")
  > EOF
  $ cat >> server/.hg/hgrc <<EOF
  > [hooks]
  > pretxnopen.movebook = python:$TESTTMP/move.py:movebookmark
  > EOF
  $ hg -R client pull -q -r 0
  $ hg -R client update -q 'desc(aa)'
  $ echo c >> client/c
  $ hg -R client commit -qAm 'cc'
  $ hg -R client log -G -T '"{desc}" {bookmarks}'
  @  "cc"
  │
  o  "aa"
  
  $ hg -R client push --to bm
  pushing rev 5db65b93a12b to destination ssh://user@dummy/server bookmark bm
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  updating bookmark bm
  remote: moved bookmark to rev 1
  remote: pushing 1 changeset:
  remote:     5db65b93a12b  cc
  remote: 2 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R server log -G -T '"{desc}" {bookmarks}'
  o  "cc" bm
  │
  @  "bb"
  │
  o  "aa"
  
  $ hg -R client log -G -T '"{desc}" {bookmarks} {remotenames}'
  @  "cc"  default/bm
  │
  o  "bb"
  │
  o  "aa"
  

Test that we still don't allow non-ff bm changes

  $ echo d > client/d
  $ hg -R client commit -qAm "dd"
  $ hg -R client log -G -T '"{desc}" {bookmarks}'
  @  "dd"
  │
  o  "cc"
  │
  o  "bb"
  │
  o  "aa"
  

  $ hg -R client push --to bm
  pushing rev 2f9755549086 to destination ssh://user@dummy/server bookmark bm
  searching for changes
  remote: moved bookmark to rev 1
  remote: pushing 1 changeset:
  remote:     2f9755549086  dd
  remote: 1 new changeset from the server will be downloaded
  remote: transaction abort! (?)
  remote: rollback completed (?)
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
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     1846eede8b68  b
  remote: 1 new changeset from the server will be downloaded
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -r 'all()' -T '{desc} {remotebookmarks} {bookmarks}'
  @  b default/master
  │
  o  a
  
  $ hg pull
  pulling from * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hg log -G -T '{desc} {remotebookmarks}'
  o  aa
  │
  │ @  b default/master
  ├─╯
  o  a
  
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
  adding changesets
  adding manifests
  adding file changes
  updating bookmark master
  remote: pushing 1 changeset:
  remote:     d2ae7f538514  b
  remote: 2 new changesets from the server will be downloaded
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -T '{desc}' -G
  @  b
  │
  o  c
  │
  o  a
  
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
  updating bookmark serverfeature
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ hg log -G -T '{shortest(node)} {bookmarks}'
  @  d2ae clientfeature
  │
  o  cb9a
  
  $ cd ../oldserver
  $ hg log -G -T '{shortest(node)} {bookmarks}'
  o  d2ae serverfeature
  │
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
  remote: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  remote: error: prechangegroup.blocknonpushrebase hook failed: this repository requires that you enable the pushrebase extension and push using 'hg push --to'
  abort: push failed on remote
  [255]

