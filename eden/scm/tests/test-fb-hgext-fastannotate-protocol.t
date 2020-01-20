#chg-compatible

  $ disable treemanifest

  $ configure dummyssh
  $ enable fastannotate
  $ setconfig fastannotate.mainbranch=@

  $ HGMERGE=true; export HGMERGE

setup the server repo

  $ hg init repo-server
  $ cd repo-server
  $ setconfig fastannotate.server=1
  $ for i in 1 2 3 4; do
  >   echo $i >> a
  >   hg commit -A -m $i a
  > done
  $ [ -d .hg/fastannotate ]
  [1]
  $ hg bookmark @
  $ cd ..

setup the local repo

  $ hg clone 'ssh://user@dummy/repo-server' repo-local -q
  $ cd repo-local
  $ setconfig fastannotate.client=1 fastannotate.clientfetchthreshold=0
  $ [ -d .hg/fastannotate ]
  [1]
  $ hg fastannotate a --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob) (?)
  remote: capabilities: * (glob)
  remote: * (glob) (?)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing 112 bytes to fastannotate/default/a.l (?)
  fastannotate: writing 94 bytes to fastannotate/default/a.m
  fastannotate: writing 112 bytes to fastannotate/default/a.l (?)
  fastannotate: a: using fast path (resolved fctx: True)
  0: 1
  1: 2
  2: 3
  3: 4

the cache could be reused and no download is necessary

  $ hg fastannotate a --debug
  fastannotate: a: using fast path (resolved fctx: True)
  0: 1
  1: 2
  2: 3
  3: 4

if the client agrees where the head of the master branch is, no re-download
happens even if the client has more commits

  $ echo 5 >> a
  $ hg commit -m 5
  $ hg bookmark -r 3 @ -f
  $ hg fastannotate a --debug
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

if the client has a different "@" (head of the master branch) and "@" is ahead
of the server, the server can detect things are unchanged and does not return
full contents (not that there is no "writing ... to fastannotate"), but the
client can also build things up on its own (causing diverge)

  $ hg bookmark -r 4 @ -f
  $ hg fastannotate a --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob) (?)
  remote: capabilities: * (glob)
  remote: * (glob) (?)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: a: 1 new changesets in the main branch
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

if the client has a different "@" which is behind the server. no download is
necessary

  $ hg fastannotate a --debug --config fastannotate.mainbranch=2
  fastannotate: a: using fast path (resolved fctx: True)
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

define fastannotate on-disk paths

  $ p1=.hg/fastannotate/default
  $ p2=../repo-server/.hg/fastannotate/default

revert bookmark change so the client is behind the server

  $ hg bookmark -r 2 @ -f

in the "fctx" mode with the "annotate" command, the client also downloads the
cache. but not in the (default) "fastannotate" mode.

  $ rm $p1/a.l $p1/a.m
  $ hg annotate a --debug | grep 'fastannotate: writing'
  [1]
  $ hg annotate a --config fastannotate.modes=fctx --debug | grep 'fastannotate: writing' | sort
  fastannotate: writing 112 bytes to fastannotate/default/a.l
  fastannotate: writing 94 bytes to fastannotate/default/a.m

the fastannotate cache (built server-side, downloaded client-side) in two repos
have the same content (because the client downloads from the server)

  $ diff $p1/a.l $p2/a.l
  $ diff $p1/a.m $p2/a.m

in the "fctx" mode, the client could also build the cache locally

  $ hg annotate a --config fastannotate.modes=fctx --debug --config fastannotate.mainbranch=4 | grep fastannotate
  fastannotate: requesting 1 files
  fastannotate: server returned
  fastannotate: a: 1 new changesets in the main branch

the server would rebuild broken cache automatically

  $ cp $p2/a.m $p2/a.m.bak
  $ echo BROKEN1 > $p1/a.m
  $ echo BROKEN2 > $p2/a.m
  $ hg fastannotate a --debug | grep 'fastannotate: writing' | sort
  fastannotate: writing 112 bytes to fastannotate/default/a.l
  fastannotate: writing 94 bytes to fastannotate/default/a.m
  $ diff $p1/a.m $p2/a.m
  $ diff $p2/a.m $p2/a.m.bak

use the "debugbuildannotatecache" command to build annotate cache

  $ rm -rf $p1 $p2
  $ hg --cwd ../repo-server debugbuildannotatecache a --debug
  fastannotate: a: 4 new changesets in the main branch
  $ hg --cwd ../repo-local debugbuildannotatecache a --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob) (?)
  remote: capabilities: * (glob)
  remote: * (glob) (?)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * (glob)
  fastannotate: writing * (glob)
  $ diff $p1/a.l $p2/a.l
  $ diff $p1/a.m $p2/a.m

with the clientfetchthreshold config option, the client can build up the cache
without downloading from the server

  $ rm -rf $p1
  $ hg fastannotate a --debug --config fastannotate.clientfetchthreshold=10
  fastannotate: a: 3 new changesets in the main branch
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

if the fastannotate directory is not writable, the fctx mode still works

  $ rm -rf $p1
  $ touch $p1
  $ hg annotate a --debug --traceback --config fastannotate.modes=fctx
  fastannotate: a: cache broken and deleted
  fastannotate: prefetch failed: * (glob)
  fastannotate: a: cache broken and deleted
  fastannotate: falling back to the vanilla annotate: * (glob)
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

with serverbuildondemand=False, the server will not build anything

  $ cat >> ../repo-server/.hg/hgrc <<EOF
  > [fastannotate]
  > serverbuildondemand=False
  > EOF
  $ rm -rf $p1 $p2
  $ hg fastannotate a --debug | grep 'fastannotate: writing'
  [1]
