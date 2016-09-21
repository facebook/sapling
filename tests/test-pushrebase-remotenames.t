  $ . $TESTDIR/require-ext.sh remotenames

Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = python "$RUNTESTDIR/dummyssh"
  > EOF

Set up server repository

  $ hg init server
  $ cd server
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > remotenames = !
  > [experimental]
  > bundle2-exp=True
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

Test that pushing to a remotename gets rebased

  $ cd server
  $ hg up -q master
  $ echo x >> a && hg commit -m "master's commit"
  $ cd ../client
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > remotenames =
  > bundle2hooks =
  > pushrebase =
  > [remotenames]
  > allownonfastforward=True
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ echo x >> b && hg commit -m "client's commit"
  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  @  1 "client's commit"
  |
  o  0 "initial" default/master
  

  $ hg push --to master
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark master
  searching for changes
  remote: pushing 1 changset:
  remote:     5c3cfb78df2f  client's commit
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  updating bookmark master

  $ hg log -G -T '{rev} "{desc}" {remotebookmarks}'
  o  3 "client's commit" default/master
  |
  o  2 "master's commit"
  |
  | @  1 "client's commit"
  |/
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
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  abort: not creating new remote bookmark
  (use --create to create a new bookmark)
  [255]

  $ hg -R client push --to newbook --create
  pushing rev 5c3cfb78df2f to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  remote: pushing 1 changset:
  remote:     5c3cfb78df2f  client's commit
  exporting bookmark newbook
  $ hg -R server book
   * master                    2:796d44dcaae0
     newbook                   3:5c3cfb78df2f
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  3 "client's commit" newbook
  |
  | o  2 "client's commit" master
  | |
  | @  1 "master's commit"
  |/
  o  0 "initial"
  
Test doing a non-fastforward bookmark move

  $ hg -R client push --to newbook -r master -f
  pushing rev 796d44dcaae0 to destination ssh://user@dummy/server bookmark newbook
  searching for changes
  no changes found
  updating bookmark newbook
  [1]
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  3 "client's commit"
  |
  | o  2 "client's commit" master newbook
  | |
  | @  1 "master's commit"
  |/
  o  0 "initial"
  

Test a push that comes with out-of-date bookmark discovery

  $ hg -R server strip -q 0 --config extensions.strip=
  $ hg -R client strip -q 0 --config extensions.strip=
  $ rm server/.hg/bookmarks*
  $ rm client/.hg/bookmarks*
  $ echo a >> server/a
  $ hg -R server commit -qAm 'aa'
  $ hg -R server bookmark bm -i
  $ echo b >> server/b
  $ hg -R server commit -qAm 'bb'
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  @  1 "bb"
  |
  o  0 "aa" bm
  

  $ cat >> $TESTTMP/move.py <<EOF
  > def movebookmark(ui, repo, **kwargs):
  >     import traceback
  >     if [f for f in traceback.extract_stack(limit=10)[:-1] if f[2] == "movebookmark"]:
  >         return
  >     import mercurial.lock as lockmod
  >     tr = None
  >     try:
  >         lock = repo.lock()
  >         tr = repo.transaction("pretxnopen.movebook")
  >         bm = repo._bookmarks
  >         bm['bm'] = repo[1].node()
  >         bm.recordchange(tr)
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
  remote: pushing 1 changset:
  remote:     5db65b93a12b  cc
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  updating bookmark bm
  $ hg -R server log -G -T '{rev} "{desc}" {bookmarks}'
  o  2 "cc" bm
  |
  @  1 "bb"
  |
  o  0 "aa"
  
  $ hg -R client log -G -T '{rev} "{desc}" {bookmarks}'
  o  3 "cc"
  |
  o  2 "bb"
  |
  | @  1 "cc"
  |/
  o  0 "aa"
  

Test that we still don't allow non-ff bm changes

  $ echo d > client/d
  $ hg -R client commit -qAm "dd"
  $ hg -R client log -G -T '{rev} "{desc}" {bookmarks}'
  @  4 "dd"
  |
  | o  3 "cc"
  | |
  | o  2 "bb"
  | |
  o |  1 "cc"
  |/
  o  0 "aa"
  

  $ hg -R client push --to bm
  pushing rev efec53e7b035 to destination ssh://user@dummy/server bookmark bm
  searching for changes
  remote: moved bookmark to rev 1
  remote: pushing 2 changsets:
  remote:     5db65b93a12b  cc
  remote:     efec53e7b035  dd
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
  > bundle2hooks =
  > pushrebase =
  > remotenames = !
  > [experimental]
  > bundle2-exp=True
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
  > bundle2hooks =
  > pushrebase =
  > remotenames =
  > [remotenames]
  > allownonfastforward=True
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ hg up master
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b >> a && hg commit -Aqm b
  $ hg push -f --to master
  pushing rev 1846eede8b68 to destination * (glob)
  searching for changes
  remote: pushing 1 changset:
  remote:     1846eede8b68  b
  updating bookmark master
  $ hg pull
  pulling from * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg log -G -T '{rev} {desc} {remotebookmarks}'
  o  2 aa
  |
  | @  1 b default/master
  |/
  o  0 a
  
  $ cd ..

Test 'hg push' with a tracking bookmark
  $ hg init trackingserver
  $ cd trackingserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > remotenames = !
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book master
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' -q ssh://user@dummy/trackingserver trackingclient
  $ cd trackingclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > remotenames =
  > [remotenames]
  > allownonfastforward=True
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ hg book feature -t default/master
  $ echo b > b && hg commit -Aqm b
  $ cd ../trackingserver
  $ echo c > c && hg commit -Aqm c
  $ cd ../trackingclient
  $ hg push
  pushing rev d2ae7f538514 to destination ssh://user@dummy/trackingserver bookmark master
  searching for changes
  remote: pushing 1 changset:
  remote:     d2ae7f538514  b
  remote: 2 new changesets from the server will be downloaded
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 2 files (+1 heads)
  updating bookmark master
  $ hg log -T '{rev} {desc}' -G
  o  3 b
  |
  o  2 c
  |
  | @  1 b
  |/
  o  0 a
  
  $ cd ..

Test push --to to a repo without pushrebase on (i.e. the default remotenames behavior)
  $ hg init oldserver
  $ cd oldserver
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > remotenames =
  > [experimental]
  > bundle2-exp=True
  > EOF
  $ echo a > a && hg commit -Aqm a
  $ hg book serverfeature
  $ cd ..
  $ hg clone --config 'extensions.remotenames=' -q ssh://user@dummy/oldserver newclient
  $ cd newclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > bundle2hooks =
  > pushrebase =
  > remotenames =
  > [experimental]
  > bundle2-exp=True
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
  
