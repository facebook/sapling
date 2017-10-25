#require killdaemons

  $ hg clone http://localhost:$HGPORT/ copy
  abort: * (glob)
  [255]
  $ test -d copy
  [1]

This server doesn't do range requests so it's basically only good for
one pull

  $ $PYTHON "$TESTDIR/dumbhttp.py" -p $HGPORT --pid dumb.pid \
  > --logfile server.log
  $ cat dumb.pid >> $DAEMON_PIDS
  $ hg init remote
  $ cd remote
  $ echo foo > bar
  $ echo c2 > '.dotfile with spaces'
  $ hg add
  adding .dotfile with spaces
  adding bar
  $ hg commit -m"test"
  $ hg tip
  changeset:   0:02770d679fb8
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  
  $ cd ..
  $ hg clone static-http://localhost:$HGPORT/remote local
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  new changesets 02770d679fb8
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd local
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 1 changesets, 2 total revisions
  $ cat bar
  foo
  $ cd ../remote
  $ echo baz > quux
  $ hg commit -A -mtest2
  adding quux

check for HTTP opener failures when cachefile does not exist

  $ rm .hg/cache/*
  $ cd ../local
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > changegroup = sh -c "printenv.py changegroup"
  > EOF
  $ hg pull
  pulling from static-http://localhost:$HGPORT/remote
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 4ac2e3648604
  changegroup hook: HG_HOOKNAME=changegroup HG_HOOKTYPE=changegroup HG_NODE=4ac2e3648604439c580c69b09ec9d93a88d93432 HG_NODE_LAST=4ac2e3648604439c580c69b09ec9d93a88d93432 HG_SOURCE=pull HG_TXNID=TXN:$ID$ HG_URL=http://localhost:$HGPORT/remote
  (run 'hg update' to get a working copy)

trying to push

  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo more foo >> bar
  $ hg commit -m"test"
  $ hg push
  pushing to static-http://localhost:$HGPORT/remote
  abort: destination does not support push
  [255]

trying clone -r

  $ cd ..
  $ hg clone -r doesnotexist static-http://localhost:$HGPORT/remote local0
  abort: unknown revision 'doesnotexist'!
  [255]
  $ hg clone -r 0 static-http://localhost:$HGPORT/remote local0
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  new changesets 02770d679fb8
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

test with "/" URI (issue747) and subrepo

  $ hg init
  $ hg init sub
  $ touch sub/test
  $ hg -R sub commit -A -m "test"
  adding test
  $ hg -R sub tag not-empty
  $ echo sub=sub > .hgsub
  $ echo a > a
  $ hg add a .hgsub
  $ hg -q ci -ma
  $ hg clone static-http://localhost:$HGPORT/ local2
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 3 changes to 3 files
  new changesets a9ebfbe8e587
  updating to branch default
  cloning subrepo sub from static-http://localhost:$HGPORT/sub
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  new changesets be090ea66256:322ea90975df
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd local2
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  3 files, 1 changesets, 3 total revisions
  checking subrepo links
  $ cat a
  a
  $ hg paths
  default = static-http://localhost:$HGPORT/

test with empty repo (issue965)

  $ cd ..
  $ hg init remotempty
  $ hg clone static-http://localhost:$HGPORT/remotempty local3
  no changes found
  updating to branch default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd local3
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  0 files, 0 changesets, 0 total revisions
  $ hg paths
  default = static-http://localhost:$HGPORT/remotempty

test with non-repo

  $ cd ..
  $ mkdir notarepo
  $ hg clone static-http://localhost:$HGPORT/notarepo local3
  abort: 'http://localhost:$HGPORT/notarepo' does not appear to be an hg repository!
  [255]

Clone with tags and branches works

  $ hg init remote-with-names
  $ cd remote-with-names
  $ echo 0 > foo
  $ hg -q commit -A -m initial
  $ echo 1 > foo
  $ hg commit -m 'commit 1'
  $ hg -q up 0
  $ hg branch mybranch
  marked working directory as branch mybranch
  (branches are permanent and global, did you want a bookmark?)
  $ echo 2 > foo
  $ hg commit -m 'commit 2 (mybranch)'
  $ hg tag -r 1 'default-tag'
  $ hg tag -r 2 'branch-tag'

  $ cd ..

  $ hg clone static-http://localhost:$HGPORT/remote-with-names local-with-names
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 5 changesets with 5 changes to 2 files (+1 heads)
  new changesets 68986213bd44:0c325bd2b5a7
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Clone a specific branch works

  $ hg clone -r mybranch static-http://localhost:$HGPORT/remote-with-names local-with-names-branch
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 2 files
  new changesets 68986213bd44:0c325bd2b5a7
  updating to branch mybranch
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Clone a specific tag works

  $ hg clone -r default-tag static-http://localhost:$HGPORT/remote-with-names local-with-names-tag
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  new changesets 68986213bd44:4ee3fcef1c80
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ killdaemons.py

List of files accessed over HTTP:

  $ cat server.log | sed -n -e 's|.*GET \(/[^ ]*\).*|\1|p' | sort -u
  /.hg/bookmarks
  /.hg/bookmarks.current
  /.hg/cache/hgtagsfnodes1
  /.hg/requires
  /.hg/store/00changelog.i
  /.hg/store/00manifest.i
  /.hg/store/data/%7E2ehgsub.i
  /.hg/store/data/%7E2ehgsubstate.i
  /.hg/store/data/a.i
  /notarepo/.hg/00changelog.i
  /notarepo/.hg/requires
  /remote-with-names/.hg/bookmarks
  /remote-with-names/.hg/bookmarks.current
  /remote-with-names/.hg/cache/branch2-served
  /remote-with-names/.hg/cache/hgtagsfnodes1
  /remote-with-names/.hg/cache/tags2-served
  /remote-with-names/.hg/localtags
  /remote-with-names/.hg/requires
  /remote-with-names/.hg/store/00changelog.i
  /remote-with-names/.hg/store/00manifest.i
  /remote-with-names/.hg/store/data/%7E2ehgtags.i
  /remote-with-names/.hg/store/data/foo.i
  /remote/.hg/bookmarks
  /remote/.hg/bookmarks.current
  /remote/.hg/cache/branch2-base
  /remote/.hg/cache/branch2-immutable
  /remote/.hg/cache/branch2-served
  /remote/.hg/cache/hgtagsfnodes1
  /remote/.hg/cache/rbc-names-v1
  /remote/.hg/cache/tags2-served
  /remote/.hg/localtags
  /remote/.hg/requires
  /remote/.hg/store/00changelog.i
  /remote/.hg/store/00manifest.i
  /remote/.hg/store/data/%7E2edotfile%20with%20spaces.i
  /remote/.hg/store/data/%7E2ehgtags.i
  /remote/.hg/store/data/bar.i
  /remote/.hg/store/data/quux.i
  /remotempty/.hg/bookmarks
  /remotempty/.hg/bookmarks.current
  /remotempty/.hg/requires
  /remotempty/.hg/store/00changelog.i
  /remotempty/.hg/store/00manifest.i
  /sub/.hg/bookmarks
  /sub/.hg/bookmarks.current
  /sub/.hg/cache/hgtagsfnodes1
  /sub/.hg/requires
  /sub/.hg/store/00changelog.i
  /sub/.hg/store/00manifest.i
  /sub/.hg/store/data/%7E2ehgtags.i
  /sub/.hg/store/data/test.i
