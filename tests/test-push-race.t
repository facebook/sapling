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
  > import errno
  > import os
  > import time
  > 
  > from mercurial import (
  >     exchange,
  >     extensions,
  >     registrar,
  > )
  > 
  > configtable = {}
  > configitem = registrar.configitem(configtable)
  > 
  > configitem('delaypush', 'ready-path',
  >     default=None,
  > )
  > configitem('delaypush', 'release-path',
  >     default=None,
  > )
  > 
  > def delaypush(orig, pushop):
  >     # notify we are done preparing
  >     ui = pushop.repo.ui
  >     readypath = ui.config('delaypush', 'ready-path')
  >     if readypath is not None:
  >         with open(readypath, 'w') as r:
  >             r.write('foo')
  >         ui.status('wrote ready: %s\n' % readypath)
  >     # now wait for the other process to be done
  >     watchpath = ui.config('delaypush', 'release-path')
  >     if watchpath is not None:
  >         ui.status('waiting on: %s\n' % watchpath)
  >         limit = 100
  >         while 0 < limit and not os.path.exists(watchpath):
  >             limit -= 1
  >             time.sleep(0.1)
  >         if limit <= 0:
  >             ui.warn('exiting without watchfile: %s' % watchpath)
  >         else:
  >             # delete the file at the end of the push
  >             def delete():
  >                 try:
  >                     os.unlink(watchpath)
  >                 except OSError as exc:
  >                     if exc.errno != errno.ENOENT:
  >                         raise
  >             ui.atexit(delete)
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
  > ssh = $PYTHON "$TESTDIR/dummyssh"
  > # simplify output
  > logtemplate = {node|short} {desc} ({branch})
  > [phases]
  > publish = no
  > [experimental]
  > evolution=true
  > [alias]
  > graph = log -G --rev 'sort(all(), "topo")'
  > EOF

We tests multiple cases:
* strict: no race detected,
* unrelated: race on unrelated heads are allowed.

#testcases strict unrelated

#if unrelated

  $ cat >> $HGRCPATH << EOF
  > [server]
  > concurrent-push-mode = check-related
  > EOF

#endif

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
  new changesets 842e2fac6304
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone ssh://user@dummy/server client-other
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 842e2fac6304
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
  new changesets a9149a1428e2
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets a9149a1428e2
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 98217d5a1659
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

#if strict
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
  
#endif
#if unrelated

(The two heads are unrelated, push should be allowed)

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

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
  
#endif

Pushing while someone creates a new head
-----------------------------------------

Pushing a new changeset while someone creates a new branch.

#  a (raced)
#  |
#  * b
#  |/
#  *

(resync-all)

#if strict

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 59e76faf78bd
  (run 'hg update' to get a working copy)

#endif
#if unrelated

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  no changes found

#endif

  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 59e76faf78bd
  (run 'hg update' to get a working copy)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets 51c544a58128
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

#if strict

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
  

#endif

#if unrelated

(The racing new head do not affect existing heads, push should go through)

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ hg -R server graph
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  
#endif

Pushing touching different named branch (same topo): new branch raced
---------------------------------------------------------------------

Pushing two children on the same head, one is a different named branch

#  a (raced, branch-a)
#  |
#  | b (default branch)
#  |/
#  *

(resync-all)

#if strict

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d9e379a8c432
  (run 'hg update' to get a working copy)

#endif
#if unrelated

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  no changes found

#endif

  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d9e379a8c432
  (run 'hg update' to get a working copy)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets d603e2c0cdd7
  (run 'hg heads .' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

(update existing head)

  $ hg -R client-other/ up 'desc("C-F")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ commit -m "C-G"

(new named branch from that existing head)

  $ hg -R client-racy/ up 'desc("C-F")'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ branch my-first-test-branch
  marked working directory as branch my-first-test-branch
  (branches are permanent and global, did you want a bookmark?)
  $ hg -R client-racy/ commit -m "C-H"

Pushing

  $ hg -R client-racy push -r 'tip' --new-branch > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip'
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ release $TESTTMP/watchfile

Check the result of the push

#if strict
  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server graph
  o  75d69cba5402 C-G (default)
  |
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  
#endif
#if unrelated

(unrelated named branches are unrelated)

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)

  $ hg -R server graph
  o  833be552cfe6 C-H (my-first-test-branch)
  |
  | o  75d69cba5402 C-G (default)
  |/
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  
#endif

The racing new head do not affect existing heads, push should go through

pushing touching different named branch (same topo): old branch raced
---------------------------------------------------------------------

Pushing two children on the same head, one is a different named branch

#  a (raced, default-branch)
#  |
#  | b (new branch)
#  |/
#  * (default-branch)

(resync-all)

#if strict

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 833be552cfe6
  (run 'hg heads .' to see heads, 'hg merge' to merge)

#endif
#if unrelated

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  no changes found

#endif

  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 833be552cfe6
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 75d69cba5402
  (run 'hg heads' to see heads)

  $ hg -R server graph
  o  833be552cfe6 C-H (my-first-test-branch)
  |
  | o  75d69cba5402 C-G (default)
  |/
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

(new named branch from one head)

  $ hg -R client-other/ up 'desc("C-G")'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ branch my-second-test-branch
  marked working directory as branch my-second-test-branch
  $ hg -R client-other/ commit -m "C-I"

(children "updating" that same head)

  $ hg -R client-racy/ up 'desc("C-G")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ commit -m "C-J"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files

  $ release $TESTTMP/watchfile

Check the result of the push

#if strict

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server graph
  o  b35ed749f288 C-I (my-second-test-branch)
  |
  o  75d69cba5402 C-G (default)
  |
  | o  833be552cfe6 C-H (my-first-test-branch)
  |/
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

#endif

#if unrelated

(unrelated named branches are unrelated)

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 1 changes to 1 files (+1 heads)

  $ hg -R server graph
  o  89420bf00fae C-J (default)
  |
  | o  b35ed749f288 C-I (my-second-test-branch)
  |/
  o  75d69cba5402 C-G (default)
  |
  | o  833be552cfe6 C-H (my-first-test-branch)
  |/
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

#endif

pushing racing push touch multiple heads
----------------------------------------

There are multiple heads, but the racing push touch all of them

#  a (raced)
#  | b
#  |/|
#  * *
#  |/
#  *

(resync-all)

#if strict

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 89420bf00fae
  (run 'hg heads .' to see heads, 'hg merge' to merge)

#endif

#if unrelated

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  no changes found

#endif

  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 89420bf00fae
  (run 'hg heads' to see heads)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets b35ed749f288
  (run 'hg heads .' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  89420bf00fae C-J (default)
  |
  | o  b35ed749f288 C-I (my-second-test-branch)
  |/
  o  75d69cba5402 C-G (default)
  |
  | o  833be552cfe6 C-H (my-first-test-branch)
  |/
  o  d9e379a8c432 C-F (default)
  |
  o  51c544a58128 C-C (default)
  |
  | o  d603e2c0cdd7 C-E (default)
  |/
  o  98217d5a1659 C-A (default)
  |
  | o  59e76faf78bd C-D (default)
  | |
  | o  a9149a1428e2 C-B (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

(merges heads)

  $ hg -R client-other/ up 'desc("C-E")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R client-other/ merge 'desc("C-D")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg -R client-other/ commit -m "C-K"

(update one head)

  $ hg -R client-racy/ up 'desc("C-D")'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo bbb >> client-racy/b
  $ hg -R client-racy/ commit -m "C-L"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 0 changes to 0 files (-1 heads)

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
  o    be705100c623 C-K (default)
  |\
  | o  d603e2c0cdd7 C-E (default)
  | |
  o |  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  | | o  51c544a58128 C-C (default)
  | |/
  o |  a9149a1428e2 C-B (default)
  | |
  | o  98217d5a1659 C-A (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

pushing raced push touch multiple heads
---------------------------------------

There are multiple heads, the raced push touch all of them

#  b
#  | a (raced)
#  |/|
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
  new changesets cac2cead0ff0
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets cac2cead0ff0
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets be705100c623
  (run 'hg update' to get a working copy)

  $ hg -R server graph
  o  cac2cead0ff0 C-L (default)
  |
  | o  be705100c623 C-K (default)
  |/|
  | o  d603e2c0cdd7 C-E (default)
  | |
  o |  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  | | o  51c544a58128 C-C (default)
  | |/
  o |  a9149a1428e2 C-B (default)
  | |
  | o  98217d5a1659 C-A (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

Creating changesets

(update existing head)

  $ echo aaa >> client-other/a
  $ hg -R client-other/ commit -m "C-M"

(merge heads)

  $ hg -R client-racy/ merge 'desc("C-K")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg -R client-racy/ commit -m "C-N"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
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
  o  6fd3090135df C-M (default)
  |
  o    be705100c623 C-K (default)
  |\
  | o  d603e2c0cdd7 C-E (default)
  | |
  +---o  cac2cead0ff0 C-L (default)
  | |
  o |  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  | | o  51c544a58128 C-C (default)
  | |/
  o |  a9149a1428e2 C-B (default)
  | |
  | o  98217d5a1659 C-A (default)
  |/
  @  842e2fac6304 C-ROOT (default)
  

racing commit push a new head behind another named branch
---------------------------------------------------------

non-continuous branch are valid case, we tests for them.

#  b (branch default)
#  |
#  o (branch foo)
#  |
#  | a (raced, branch default)
#  |/
#  * (branch foo)
#  |
#  * (branch default)

(resync-all + other branch)

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 866a66e18630
  (run 'hg update' to get a working copy)

(creates named branch on head)

  $ hg -R ./server/ up 'desc("C-N")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R ./server/ branch other
  marked working directory as branch other
  $ hg -R ./server/ ci -m "C-Z"
  $ hg -R ./server/ up null
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved

(sync client)

  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 866a66e18630:55a6f1c01b48
  (run 'hg update' to get a working copy)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files (+1 heads)
  new changesets 6fd3090135df:55a6f1c01b48
  (run 'hg heads .' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

Creating changesets

(update default head through another named branch one)

  $ hg -R client-other/ up 'desc("C-Z")'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ commit -m "C-O"
  $ echo aaa >> client-other/a
  $ hg -R client-other/ branch --force default
  marked working directory as branch default
  $ hg -R client-other/ commit -m "C-P"
  created new head

(update default head)

  $ hg -R client-racy/ up 'desc("C-Z")'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-other/a
  $ hg -R client-racy/ branch --force default
  marked working directory as branch default
  $ hg -R client-racy/ commit -m "C-Q"
  created new head

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 2 changesets with 1 changes to 1 files

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
  o  1b58ee3f79e5 C-P (default)
  |
  o  d0a85b2252a9 C-O (other)
  |
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

raced commit push a new head behind another named branch
---------------------------------------------------------

non-continuous branch are valid case, we tests for them.

#  b (raced branch default)
#  |
#  o (branch foo)
#  |
#  | a (branch default)
#  |/
#  * (branch foo)
#  |
#  * (branch default)

(resync-all)

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets b0ee3d6f51bc
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets b0ee3d6f51bc
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 1 changes to 1 files (+1 heads)
  new changesets d0a85b2252a9:1b58ee3f79e5
  (run 'hg heads .' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  b0ee3d6f51bc C-Q (default)
  |
  | o  1b58ee3f79e5 C-P (default)
  | |
  | o  d0a85b2252a9 C-O (other)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

Creating changesets

(update 'other' named branch head)

  $ hg -R client-other/ up 'desc("C-P")'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ branch --force other
  marked working directory as branch other
  $ hg -R client-other/ commit -m "C-R"
  created new head

(update 'other named brnach through a 'default' changeset')

  $ hg -R client-racy/ up 'desc("C-P")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ commit -m "C-S"
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ branch --force other
  marked working directory as branch other
  $ hg -R client-racy/ commit -m "C-T"
  created new head

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
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
  o  de7b9e2ba3f6 C-R (other)
  |
  o  1b58ee3f79e5 C-P (default)
  |
  o  d0a85b2252a9 C-O (other)
  |
  | o  b0ee3d6f51bc C-Q (default)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

raced commit push a new head obsoleting the one touched by the racing push
--------------------------------------------------------------------------

#  b (racing)
#  |
#  ø⇠◔ a (raced)
#  |/
#  *

(resync-all)

  $ hg -R ./server pull ./client-racy
  pulling from ./client-racy
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files (+1 heads)
  new changesets 2efd43f7b5ba:3d57ed3c1091
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files (+1 heads)
  new changesets 2efd43f7b5ba:3d57ed3c1091
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets de7b9e2ba3f6
  (run 'hg heads' to see heads, 'hg merge' to merge)

  $ hg -R server graph
  o  3d57ed3c1091 C-T (other)
  |
  o  2efd43f7b5ba C-S (default)
  |
  | o  de7b9e2ba3f6 C-R (other)
  |/
  o  1b58ee3f79e5 C-P (default)
  |
  o  d0a85b2252a9 C-O (other)
  |
  | o  b0ee3d6f51bc C-Q (default)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

Creating changesets and markers

(continue existing head)

  $ hg -R client-other/ up 'desc("C-Q")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aaa >> client-other/a
  $ hg -R client-other/ commit -m "C-U"

(new topo branch obsoleting that same head)

  $ hg -R client-racy/ up 'desc("C-Z")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-racy/a
  $ hg -R client-racy/ branch --force default
  marked working directory as branch default
  $ hg -R client-racy/ commit -m "C-V"
  created new head
  $ ID_Q=`hg -R client-racy log -T '{node}\n' -r 'desc("C-Q")'`
  $ ID_V=`hg -R client-racy log -T '{node}\n' -r 'desc("C-V")'`
  $ hg -R client-racy debugobsolete $ID_Q $ID_V
  obsoleted 1 changesets

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 0 changes to 0 files

  $ release $TESTTMP/watchfile

Check the result of the push

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server debugobsolete
  $ hg -R server graph
  o  a98a47d8b85b C-U (default)
  |
  o  b0ee3d6f51bc C-Q (default)
  |
  | o  3d57ed3c1091 C-T (other)
  | |
  | o  2efd43f7b5ba C-S (default)
  | |
  | | o  de7b9e2ba3f6 C-R (other)
  | |/
  | o  1b58ee3f79e5 C-P (default)
  | |
  | o  d0a85b2252a9 C-O (other)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

racing commit push a new head obsoleting the one touched by the raced push
--------------------------------------------------------------------------

(mirror test case of the previous one

#  a (raced branch default)
#  |
#  ø⇠◔ b (racing)
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
  1 new obsolescence markers
  obsoleted 1 changesets
  new changesets 720c5163ecf6
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-other pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  1 new obsolescence markers
  obsoleted 1 changesets
  new changesets 720c5163ecf6
  (run 'hg heads .' to see heads, 'hg merge' to merge)
  $ hg -R ./client-racy pull
  pulling from ssh://user@dummy/server
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets a98a47d8b85b
  (run 'hg update' to get a working copy)

  $ hg -R server debugobsolete
  b0ee3d6f51bc4c0ca6d4f2907708027a6c376233 720c5163ecf64dcc6216bee2d62bf3edb1882499 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg -R server graph
  o  720c5163ecf6 C-V (default)
  |
  | o  a98a47d8b85b C-U (default)
  | |
  | x  b0ee3d6f51bc C-Q (default)
  |/
  | o  3d57ed3c1091 C-T (other)
  | |
  | o  2efd43f7b5ba C-S (default)
  | |
  | | o  de7b9e2ba3f6 C-R (other)
  | |/
  | o  1b58ee3f79e5 C-P (default)
  | |
  | o  d0a85b2252a9 C-O (other)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  

Creating changesets and markers

(new topo branch obsoleting that same head)

  $ hg -R client-other/ up 'desc("C-Q")'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bbb >> client-other/a
  $ hg -R client-other/ branch --force default
  marked working directory as branch default
  $ hg -R client-other/ commit -m "C-W"
  created new head
  $ ID_V=`hg -R client-other log -T '{node}\n' -r 'desc("C-V")'`
  $ ID_W=`hg -R client-other log -T '{node}\n' -r 'desc("C-W")'`
  $ hg -R client-other debugobsolete $ID_V $ID_W
  obsoleted 1 changesets

(continue the same head)

  $ echo aaa >> client-racy/a
  $ hg -R client-racy/ commit -m "C-X"

Pushing

  $ hg -R client-racy push -r 'tip' > ./push-log 2>&1 &

  $ waiton $TESTTMP/readyfile

  $ hg -R client-other push -fr 'tip' --new-branch
  pushing to ssh://user@dummy/server
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 1 changesets with 0 changes to 1 files (+1 heads)
  remote: 1 new obsolescence markers
  remote: obsoleted 1 changesets

  $ release $TESTTMP/watchfile

Check the result of the push

  $ cat ./push-log
  pushing to ssh://user@dummy/server
  searching for changes
  wrote ready: $TESTTMP/readyfile
  waiting on: $TESTTMP/watchfile
  abort: push failed:
  'repository changed while pushing - please try again'

  $ hg -R server debugobsolete
  b0ee3d6f51bc4c0ca6d4f2907708027a6c376233 720c5163ecf64dcc6216bee2d62bf3edb1882499 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  720c5163ecf64dcc6216bee2d62bf3edb1882499 39bc0598afe90ab18da460bafecc0fa953b77596 0 (Thu Jan 01 00:00:00 1970 +0000) {'user': 'test'}
  $ hg -R server graph --hidden
  o  39bc0598afe9 C-W (default)
  |
  | o  a98a47d8b85b C-U (default)
  |/
  x  b0ee3d6f51bc C-Q (default)
  |
  | o  3d57ed3c1091 C-T (other)
  | |
  | o  2efd43f7b5ba C-S (default)
  | |
  | | o  de7b9e2ba3f6 C-R (other)
  | |/
  | o  1b58ee3f79e5 C-P (default)
  | |
  | o  d0a85b2252a9 C-O (other)
  |/
  | x  720c5163ecf6 C-V (default)
  |/
  o  55a6f1c01b48 C-Z (other)
  |
  o    866a66e18630 C-N (default)
  |\
  +---o  6fd3090135df C-M (default)
  | |
  | o  cac2cead0ff0 C-L (default)
  | |
  o |  be705100c623 C-K (default)
  |\|
  o |  d603e2c0cdd7 C-E (default)
  | |
  | o  59e76faf78bd C-D (default)
  | |
  | | o  89420bf00fae C-J (default)
  | | |
  | | | o  b35ed749f288 C-I (my-second-test-branch)
  | | |/
  | | o  75d69cba5402 C-G (default)
  | | |
  | | | o  833be552cfe6 C-H (my-first-test-branch)
  | | |/
  | | o  d9e379a8c432 C-F (default)
  | | |
  +---o  51c544a58128 C-C (default)
  | |
  | o  a9149a1428e2 C-B (default)
  | |
  o |  98217d5a1659 C-A (default)
  |/
  o  842e2fac6304 C-ROOT (default)
  
