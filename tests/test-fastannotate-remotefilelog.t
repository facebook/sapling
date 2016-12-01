  $ PYTHONPATH=$TESTDIR/../:$PYTHONPATH
  $ export PYTHONPATH

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
  fastannotate: x: remotefilelog prefetch disabled
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  2: z

The second time, the existing cache would be reused

  $ hg blame x --debug
  fastannotate: x: remotefilelog prefetch disabled
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  2: z

Nuke the fastannotate cache

  $ rm -rf .hg/fastannotate

With a higher clientfetchthreshold, the client would prefetch file contents

  $ hg blame x --debug --config fastannotate.clientfetchthreshold=10
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
  fastannotate: x: remotefilelog prefetch disabled
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  $ hg blame x -r 2 --debug
  fastannotate: x: remotefilelog prefetch disabled
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
  1: y
  2: z
  $ hg blame x -r 0 --debug
  fastannotate: x: remotefilelog prefetch disabled
  fastannotate: x: using fast path (resolved fctx: True)
  0: x
