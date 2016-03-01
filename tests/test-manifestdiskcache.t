Setup

  $ PYTHONPATH=$TESTDIR/..:$PYTHONPATH
  $ export PYTHONPATH

Test functionality is present

  $ mkdir create_on_commit
  $ cd create_on_commit
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > manifestdiskcache=
  > [manifestdiskcache]
  > logging=True
  > enabled=True
  > EOF
  $ checkabsent() {
  > ([ -f $1 ] && echo "FAIL") || echo "OK"
  > }
  $ checkpresent() {
  > ([ -f $1 ] && echo "OK") || echo "FAIL"
  > }
  $ echo "abcabc" > abcabc
  $ hg add abcabc
  $ hg commit -m "testing 123"
  $ checkpresent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ echo "defdef" > defdef
  $ hg add defdef
  $ hg commit -m "testing 456"
  $ checkpresent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ hg diff -r 0 --nodates
  diff -r 53f12ffb3d86 defdef
  --- /dev/null
  +++ b/defdef
  @@ -0,0 +1,1 @@
  +defdef
  $ cd ..

Test that we prune the cache.

  $ mkdir cache_prune
  $ cd cache_prune
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > manifestdiskcache=
  > [manifestdiskcache]
  > logging=True
  > enabled=True
  > EOF
  $ echo "abcabc" > abcabc
  $ hg add abcabc
  $ hg commit -m "testing 123"
  $ checkpresent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ echo "defdef" > defdef
  $ hg add defdef
  $ hg commit -m "testing 456"
  $ checkpresent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ echo "ghighi" > ghighi
  $ hg add ghighi
  $ hg commit -m "testing 789"
# the first two commits won't be accessed in subsequent commands, and as
# such, should be pruned.  the third commit will still be accessed when
# creating the fourth commit.  we wait 2 seconds because that's resolution
# of atime on windows.
  $ sleep 2
  $ cat >> .hg/hgrc << EOF
  > [manifestdiskcache]
  > cache-size=431
  > runs-between-prunes=1
  > pinned-revsets=
  > enabled=True
  > EOF
  $ echo "jkljkl" > jkljkl
  $ hg add jkljkl
  $ hg commit -m "testing 0ab"
# ensure the prune command completes before we read out the disk.
  $ sleep 1
  $ checkabsent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ checkabsent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ checkpresent .hg/store/manifestdiskcache/fd/cf/fdcfc1aafe7a6dfe64bbe8358eefd5bd22ca9fb6
  OK
  $ checkpresent .hg/store/manifestdiskcache/76/03/76035e7b5645d9b4ed6a3b904b23cd7592fdd01a
  OK
  $ cd ..

Test that prunes happen correctly with --repository/-R

  $ mkdir cache_prune_repository
  $ cd cache_prune_repository
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > manifestdiskcache=
  > [manifestdiskcache]
  > logging=True
  > enabled=True
  > runs-between-prunes=1
  > EOF
  $ echo "abcabc" > abcabc
  $ hg add abcabc
  $ hg commit -m "testing 123"
  $ checkpresent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ echo "defdef" > defdef
  $ hg add defdef
  $ hg commit -m "testing 456"
  $ checkpresent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ echo "ghighi" > ghighi
  $ hg add ghighi
  $ cd ..
  $ hg -R cache_prune_repository commit -m "testing 789"

Test that a corrupt cache does not interfere with correctness.

  $ mkdir corrupt_cache
  $ cd corrupt_cache
  $ hg init
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > manifestdiskcache=
  > [manifestdiskcache]
  > enabled=True
  > EOF
  $ echo "abcabc" > abcabc
  $ hg add abcabc
  $ hg commit -m "testing 123"
  $ checkpresent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ echo "defdef" > defdef
  $ hg add defdef
  $ hg commit -m "testing 456"
  $ checkpresent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ echo "garbage" > .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  $ echo "garbage" > .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  $ hg diff -r 0 --nodates
  diff -r 53f12ffb3d86 defdef
  --- /dev/null
  +++ b/defdef
  @@ -0,0 +1,1 @@
  +defdef
  $ cd ..

Test that we can pin a revision in the cache.

  $ mkdir cache_pinning
  $ cd cache_pinning
  $ hg init
  $ echo "abcabc" > abcabc
  $ hg add abcabc
  $ hg commit -m "testing 123"
  $ echo "defdef" > defdef
  $ hg add defdef
  $ hg commit -m "testing 456"
  $ checkabsent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ checkabsent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
  $ cat >> .hg/hgrc << EOF
  > [extensions]
  > manifestdiskcache=
  > [manifestdiskcache]
  > cache-size=0
  > runs-between-prunes=1
  > enabled=True
  > EOF
  $ hg diff -r ".^" --nodates
  diff -r 53f12ffb3d86 defdef
  --- /dev/null
  +++ b/defdef
  @@ -0,0 +1,1 @@
  +defdef
  $ sleep 1
  $ checkabsent .hg/store/manifestdiskcache/ce/e3/cee32e58a3ba8300f0a7f0d4d9a014c98cc2fc33
  OK
  $ checkpresent .hg/store/manifestdiskcache/8a/85/8a854c1c1a950742983621c0632c0828e0fd8e12
  OK
