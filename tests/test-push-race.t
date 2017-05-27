============================================================================================
Test cases where there are race condition between two clients pushing to the same repository
============================================================================================

This file tests cases where two clients push to a server at the same time. The
"raced" client is done preparing it push bundle when the "racing" client
perform its push. The "raced" client starts its actual push after the "racing"
client push is fully complete.

A set of extension and shell functions ensures this scheduling.

  $ cat >> delaypush.py << EOF
  > """small extension orchestrate push race
  > 
  > Client with the extensions will create a file when ready and get stuck until
  > a file is created."""
  > 
  > import atexit
  > import errno
  > import os
  > import time
  > 
  > from mercurial import (
  >     exchange,
  >     extensions,
  > )
  > 
  > def delaypush(orig, pushop):
  >     # notify we are done preparing
  >     readypath = pushop.repo.ui.config('delaypush', 'ready-path', None)
  >     if readypath is not None:
  >         with open(readypath, 'w') as r:
  >             r.write('foo')
  >         pushop.repo.ui.status('wrote ready: %s\n' % readypath)
  >     # now wait for the other process to be done
  >     watchpath = pushop.repo.ui.config('delaypush', 'release-path', None)
  >     if watchpath is not None:
  >         pushop.repo.ui.status('waiting on: %s\n' % watchpath)
  >         limit = 100
  >         while 0 < limit and not os.path.exists(watchpath):
  >             limit -= 1
  >             time.sleep(0.1)
  >         if limit <= 0:
  >             repo.ui.warn('exiting without watchfile: %s' % watchpath)
  >         else:
  >             # delete the file at the end of the push
  >             def delete():
  >                 try:
  >                     os.unlink(watchpath)
  >                 except OSError as exc:
  >                     if exc.errno != errno.ENOENT:
  >                         raise
  >             atexit.register(delete)
  >     return orig(pushop)
  > 
  > def uisetup(ui):
  >     extensions.wrapfunction(exchange, '_pushbundle2', delaypush)
  > EOF

  $ waiton () {
  >     # wait for a file to be created (then delete it)
  >     count=100
  >     while [ ! -f $1 ] ;
  >     do
  >         sleep 0.1;
  >         count=`expr $count - 1`;
  >         if [ $count -lt 0 ];
  >         then
  >              break
  >         fi;
  >     done
  >     [ -f $1 ] || echo "ready file still missing: $1"
  >     rm -f $1
  > }

  $ release () {
  >     # create a file and wait for it be deleted
  >     count=100
  >     touch $1
  >     while [ -f $1 ] ;
  >     do
  >         sleep 0.1;
  >         count=`expr $count - 1`;
  >         if [ $count -lt 0 ];
  >         then
  >              break
  >         fi;
  >     done
  >     [ ! -f $1 ] || echo "delay file still exist: $1"
  > }

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = python "$TESTDIR/dummyssh"
  > # simplify output
  > logtemplate = {node|short} {desc} ({branch})
  > [alias]
  > graph = log -G --rev 'sort(all(), "topo")'
  > EOF

Setup
-----

create a repo with one root

  $ hg init server
  $ cd server
  $ echo root > root
  $ hg ci -Am "C-ROOT"
  adding root
  $ cd ..

clone it in two clients

  $ hg clone ssh://user@dummy/server client-racy
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone ssh://user@dummy/server client-other
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

setup one to allow race on push

  $ cat >> client-racy/.hg/hgrc << EOF
  > [extensions]
  > delaypush = $TESTTMP/delaypush.py
  > [delaypush]
  > ready-path = $TESTTMP/readyfile
  > release-path = $TESTTMP/watchfile
  > EOF

Simple race, both try to push to the server at the same time
------------------------------------------------------------

Both try to replace the same head

#  a
#  | b
#  |/
#  *

Creating changesets

  $ echo b > client-other/a
  $ hg -R client-other/ add client-other/a
  $ hg -R client-other/ commit -m "C-A"
  $ echo b > client-racy/b
  $ hg -R client-racy/ add client-racy/b
  $ hg -R client-racy/ commit -m "C-B"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -r 'tip'
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ release $TESTTMP/watchfile

Check the result of the push

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server graph
  o  98217d5a1659 C-A (default)
  |
  @  842e2fac6304 C-ROOT (default)
  

Pushing on two different heads
------------------------------

Both try to replace a different head

#  a b
#  | |
#  * *
#  |/
#  *

(resync-all)

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  a9149a1428e2 C-B (default)
  |
  | o  98217d5a1659 C-A (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

  $ echo aa >> client-other/a
  $ hg -R client-other/ commit -m "C-C"
  $ echo bb >> client-racy/b
  $ hg -R client-racy/ commit -m "C-D"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -r 'tip'
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ release $TESTTMP/watchfile

Check the result of the push

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server graph
  o  51c544a58128 C-C (default)
  |
  o  98217d5a1659 C-A (default)
  |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  
Pushing while someone creates a new head
-----------------------------------------

Pushing a new changeset while someone creates a new branch.

#  a (raced)
#  |
#  * b
#  |/
#  *

(resync-all)

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

  $ hg -R server graph
  o  59e76faf78bd C-D (default)
  |
  o  a9149a1428e2 C-B (default)
  |
  | o  51c544a58128 C-C (default)
  | |
  | o  98217d5a1659 C-A (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

(new head)

  $ hg -R client-other/ up 'desc("C-A")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ commit -m "C-E"
  created new head

(children of existing head)

  $ hg -R client-racy/ up 'desc("C-C")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ commit -m "C-F"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip'
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)

  $ release $TESTTMP/watchfile

Check the result of the push

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server graph
  o  d603e2c0cdd7 C-E (default)
  |
  | o  51c544a58128 C-C (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  
