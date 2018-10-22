
  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > fastannotate=
  > [fastannotate]
  > mainbranch=default
  > modes=fctx
  > server=True
  > EOF

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo x > x
  $ hg commit -qAm x
  $ echo y >> x
  $ hg commit -qAm y
  $ echo z >> x
  $ hg commit -qAm z
  $ echo a > a
  $ hg commit -qAm a

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)
  $ cd shallow

Setup fastannotate

  $ cat >> .hg/hgrc << EOS
  > [fastannotate]
  > clientfetchthreshold = 0
  > EOS

Test blame

  $ hg blame x --debug
  fastannotate: using remotefilelog connection pool
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  2: z

The second time, the existing cache would be reused

  $ hg blame x --debug
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  2: z

Nuke the fastannotate cache

  $ rm -rf .hg/fastannotate

With a higher clientfetchthreshold, the client would prefetch file contents

  $ hg blame x --debug --config fastannotate.clientfetchthreshold=10
  remotefilelog: prefetching 2 files for annotate
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending getfiles command
  fastannotate: x: 3 new changesets in the main branch
  0: x
  1: y
  2: z
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob)

And it can reuse the annotate cache for each revision

  $ hg blame x -r 1 --debug
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: False)
  0: x
  1: y
  $ hg blame x -r 2 --debug
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: False)
  0: x
  1: y
  2: z
  $ hg blame x -r 0 --debug
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: False)
  0: x

More commits, prepared for the next test

  $ for i in 1 2; do
  >   echo $i >> ../master/x
  >   hg --cwd ../master commit -A x -m $i
  > done

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets 5b59ba03d68c:982515c3e88c
  (run 'hg update' to get a working copy)

Update to tip to predownload the remotefilelog ancestor map

  $ hg update tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Fastannotate will donate its sshpeer to remotefilelog:

  $ hg blame x -r 'tip^' --debug
  sending getfiles command (?)
  sending getfiles command (?)
  fastannotate: using remotefilelog connection pool
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  sending getfiles command
  remotefilelog: prefetching 0 files for annotate
  fastannotate: x: using fast path (resolved fctx: False)
  0: x
  1: y
  2: z
  4: 1
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Prepare a side branch

  $ hg --cwd ../master update 'tip^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd ../master branch -q side
  $ for i in 3 4 5; do
  >   echo $i >> ../master/x
  >   hg --cwd ../master commit -A x -m $i
  > done

Fastannotate teaches remotefilelog to only prefetch the side branch
(remotefilelog will first download ancestormap, which is 1 fetch)

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 0 changes to 0 files (+1 heads)
  new changesets d206b6c9326b:7e197120ae8c
  (run 'hg heads' to see heads)

  $ hg blame x -r 'side' --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending getfiles command
  remotefilelog: prefetching 3 files for annotate
  sending getfiles command
  0: x
  1: y
  2: z
  4: 1
  6: 3
  7: 4
  8: 5
  3 files fetched over 2 fetches - (3 misses, 0.00% hit ratio) over * (glob)

With fastannotate cache nuked, SSH connection could still be reused, and
remotefilelog only tries to fetch the side branch, but since they are fetched
before, no real "getfiles" happens

  $ rm -rf .hg/fastannotate
  $ hg blame x -r 'side' --debug
  fastannotate: using remotefilelog connection pool
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  remotefilelog: prefetching 3 files for annotate
  0: x
  1: y
  2: z
  4: 1
  6: 3
  7: 4
  8: 5

When fastannotate is testing clientfetchthreshold, it may trigger a fetch file
to get ancestors, in that case, remotefilelog starts sshpeer earlier than
fastannotate, and fastannotate could "steal" sshpeer from remotefilelog, then
remotefilelog could also reuse the same sshpeer afterwards.

In the below case, the first getfiles is for the ancestormap for the main
(default) branch, the second getfiles is for the side branch file contents.

  $ hg --cwd ../master update default -q
  $ for i in 6 7; do
  >   echo $i >> ../master/x
  >   hg --cwd ../master commit -A x -m $i
  >   [ $i = 6 ] && hg --cwd ../master update side -q
  > done || true

  $ hg pull -q
  $ rm -rf .hg/fastannotate

  $ echo 'clientfetchthreshold = 2' >> .hg/hgrc
  $ hg blame x -r 'side' --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending getfiles command
  fastannotate: using remotefilelog connection pool
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  fastannotate: writing * bytes to fastannotate/default/x.* (glob)
  sending getfiles command
  remotefilelog: prefetching 4 files for annotate
   0: x
   1: y
   2: z
   4: 1
   6: 3
   7: 4
   8: 5
  10: 7
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over * (glob)

If fastannotate.clientsharepeer is False, fastannotate does not donate its peer

  $ echo 'clientsharepeer = False' >> .hg/hgrc

  $ hg --cwd ../master update side -q
  $ echo 8 >> ../master/x
  $ hg --cwd ../master commit -A x -m 8

  $ hg pull -q
  $ rm -rf .hg/fastannotate

  $ hg blame x -r 'side' --debug 2>&1 | grep running
  running * (glob)
  running * (glob)

If fastannotate.clientsharepeer is False, fastannotate does not steal sshpeer

  $ hg --cwd ../master update default -q
  $ echo 9 >> ../master/x
  $ hg --cwd ../master commit -A x -m 9

  $ hg pull -q
  $ rm -rf .hg/fastannotate

  $ hg blame x -r 'default' --debug 2>&1 | grep running
  running * (glob)
  running * (glob)

Another complex case to test prefetch filtering

  $ cd ..
  $ hginit master2
  $ cd master2
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > EOF
  $ echo 1 > a
  $ hg commit -m 1 -A a
  $ hg update null -q
  $ echo 2 > a
  $ hg commit -m 2 -A a -q
  $ hg update 1 -q
  $ hg merge 0 -q
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  [1]
  $ $RUNTESTDIR/seq.py 1 3 > a
  $ hg resolve -m a -q
  $ hg commit -m merge
  $ echo 4 >> a
  $ hg commit -m 4 -A a -q
  $ hg update '.^' -q
  $ hg branch side -q
  $ { echo 0; cat a; } > a2 && mv a2 a
  $ hg commit -m 5 -A a -q
  $ hg branch side2 -q
  $ hg merge 3 -q
  $ hg commit -m merge2
  $ hg log -G -T '{rev}: {desc} ({branch})'
  @    5: merge2 (side2)
  |\
  | o  4: 5 (side)
  | |
  o |  3: 4 (default)
  |/
  o    2: merge (default)
  |\
  | o  1: 2 (default)
  |
  o  0: 1 (default)
  
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master2 shallow2 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd shallow2
  $ cat >> .hg/hgrc << EOS
  > [fastannotate]
  > clientfetchthreshold = 0
  > EOS

Annotating 3, no prefetch is needed.

  $ hg log -r . -T '{rev}\n'
  3
  $ hg annotate a -r 3 --debug
  fastannotate: using remotefilelog connection pool
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  fastannotate: requesting 1 files
  sending batch command
  fastannotate: server returned
  fastannotate: writing * bytes to fastannotate/default/a.* (glob)
  fastannotate: writing * bytes to fastannotate/default/a.* (glob)
  remotefilelog: prefetching 0 files for annotate
  fastannotate: a: using fast path (resolved fctx: False)
  0: 1
  1: 2
  2: 3
  3: 4

  $ hg up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Annotating 5, 4 and 2 (joint point) will be fetched.

  $ hg annotate a -r 5 --debug
  remotefilelog: prefetching 3 files for annotate
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: * (glob)
  sending getfiles command
  4: 0
  0: 1
  1: 2
  2: 3
  3: 4
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
