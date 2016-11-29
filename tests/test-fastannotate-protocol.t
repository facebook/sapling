  $ extpath=`dirname $TESTDIR`
  $ PYTHONPATH=$extpath:$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = $PYTHON "$TESTDIR/dummyssh"
  > [extensions]
  > fastannotate=
  > [fastannotate]
  > mainbranch=@
  > EOF

  $ HGMERGE=true; export HGMERGE

setup the server repo

  $ hg init repo-server
  $ cd repo-server
  $ cat >> .hg/hgrc << EOF
  > [fastannotate]
  > server=1
  > EOF
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
  $ cat >> .hg/hgrc << EOF
  > [fastannotate]
  > client=1
  > EOF
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
full contents (not that there is no "writing ... to fastannotate")

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
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

if the client has a different "@" which is behind the server. no download is
necessary

  $ hg fastannotate a --debug --config fastannotate.mainbranch=2
  0: 1
  1: 2
  2: 3
  3: 4
  4: 5

the fastannotate cache is built in both repos, and they are the same

  $ p1=.hg/fastannotate/default
  $ p2=../repo-server/.hg/fastannotate/default
  $ diff $p1/a.l $p2/a.l
  $ diff $p1/a.m $p2/a.m

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
