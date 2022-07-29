#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ CACHEDIR=`pwd`/hgcache

  $ . "$TESTDIR/library.sh"

  $ enable remotenames
  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > EOF
  $ mkdir dir
  $ echo x > dir/x
  $ hg commit -qAm 'add x'
  $ mkdir subdir
  $ echo z > subdir/z
  $ hg commit -qAm 'add subdir/z'
  $ echo x >> dir/x
  $ hg commit -Am 'modify x'
  $ hg bookmark master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > name=master
  > cachepath=$CACHEDIR
  > 
  > [treemanifest]
  > server=True
  > sendtrees=True
  > EOF

  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master client
  streaming all changes
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  1 files to transfer, 358 bytes of data
  transferred 358 bytes in 0.0 seconds (350 KB/sec)
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  updating to branch default
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ cd master

  $ cd ../client
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > reponame = master
  > prefetchdays=0
  > cachepath = $CACHEDIR
  > EOF

Test prefetch
  $ hg prefetch -r 'desc("add x")' -r 'desc("add subdir/z")' -r 'desc("modify x")'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  2 trees fetched over 0.00s
  fetching tree 'dir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s

TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test prefetch with base node (subdir/ shouldn't show up in the pack)
  $ rm -rf $CACHEDIR/master

Multiple trees are fetched in this case because the file prefetching code path
requires tree manifest for the base commit.

  $ hg prefetch -r '2' --base '1'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  2 trees fetched over * (glob)
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test prefetching when a draft commit is marked public
  $ mkdir $TESTTMP/cachedir.bak
  $ mv $CACHEDIR/* $TESTTMP/cachedir.bak

- Create a draft commit, and force it to be public
  $ hg prefetch -r .
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  $ echo foo > foo
  $ hg commit -Aqm 'add foo'
  $ hg debugmakepublic -r .
  $ hg log -G -T '{phase} {manifest}'
  @  public c27238bca92c2e3a599ce9f27fe799ed0d79af82
  │
  o  public 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  │
  o  public e445299a39f9006c2aec78dcc04dceeb102252b2
  │
  o  public 287ee6e53d4fbc5fab2157eb0383fdff1c3277c8
  
- Add remotenames for the remote heads
  $ hg pull --config extensions.remotenames=
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found

- Attempt to download the latest server commit. Verify there's no error about a
- missing manifest from the server.
  $ clearcache
  $ hg status --change 'desc("modify x")' --config extensions.remotenames=
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  M dir/x
  $ hg debugstrip -r 'desc("add foo")'
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd (?)
  1 trees fetched over 0.00s (?)
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ clearcache
  $ mv $TESTTMP/cachedir.bak/* $CACHEDIR

Test auto prefetch during normal access
  $ rm -rf $CACHEDIR/master
|| ( exit 1 ) is needed because ls on OSX and Linux exits differently
  $ ls $CACHEDIR/master/packs/manifests || ( exit 1 )
  * $ENOENT$ (glob)
  [1]
  $ hg log -r tip --stat --pager=off
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  commit:      311cac64787d
  bookmark:    default/master
  hoistedname: master
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify x
  
   dir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
Test that auto prefetch scans up the changelog for base trees
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r 'tip^'
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r tip
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
- Only 2 of the 3 trees from tip^ are downloaded as part of --stat's fetch
  $ hg log -r tip --stat --pager=off > /dev/null
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  fetching tree 'dir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s

Test auto prefetch during pull

- Prefetch everything
  $ echo a >> a
  $ hg commit -Aqm 'draft commit that shouldnt affect prefetch'
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=10 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching trees for 3 commits
  * trees fetched over * (glob)
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

  $ hg debugstrip -q -r 'draft()'
  2 trees fetched over 0.00s (?)

- Prefetch just the top manifest (but the full one)
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for 311cac64787d
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

- Prefetch commit 1 then minimally prefetch commit 2
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r 1
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' e445299a39f9006c2aec78dcc04dceeb102252b2
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  $ hg pull --config treemanifest.pullprefetchcount=1 --traceback
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for 311cac64787d
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test prefetching certain revs during pull
  $ cd ../master
  $ echo x >> dir/x
  $ hg commit -qm "modify dir/x a third time"
  $ echo x >> dir/x
  $ hg commit -qm "modify dir/x a fourth time"

  $ cd ../client
  $ rm -rf $CACHEDIR/master
  $ hg pull --config treemanifest.pullprefetchrevs='tip~2'
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  prefetching tree for 311cac64787d
  fetching tree '' 22febde2554a1c6f8e4d8052a0501e3d895d73d9
  1 trees fetched over 0.00s
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

- Test prefetching only the new tree parts for a commit who's parent tree is not
- downloaded already. Note that subdir/z was not downloaded this time.
  $ hg pull --config treemanifest.pullprefetchrevs='tip'
  pulling from ssh://user@dummy/master
  searching for changes
  no changes found
  prefetching tree for 47bb1c5075af
  fetching tree '' fe0e17e3157e05d7c36f025f10597357a41ef7f9
  1 trees fetched over 0.00s
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Test that prefetch refills just part of a tree when the cache is deleted

  $ echo >> dir/x
  $ hg commit -m 'edit x locally'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree 'dir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b
  1 trees fetched over 0.00s
  $ rm -rf $CACHEDIR/master/*
  $ hg cat subdir/z
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over * (glob)
  z
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)

Test prefetch non-parent commits with no base node (should fetch minimal
trees - in this case 3 trees for commit 2, and 2 for commit 4 despite it having
3 directories)
  $ rm -rf $CACHEDIR/master
  $ hg prefetch -r '2 + 4'
  3 files fetched over 1 fetches - (3 misses, 0.00% hit ratio) over * (glob) (?)
  2 trees fetched over 0.00s
  2 trees fetched over 0.00s
  fetching tree 'dir' bf22bc15398b5293cabbeef06bba44e8a2cc215c
  1 trees fetched over 0.00s

Test prefetching with no options works. The expectation is to prefetch the stuff
required for working with the draft commits which happens to be only revision 5
in this case.

  $ rm -rf $CACHEDIR/master

The tree prefetching code path fetches no trees for revision 5. However, the
file prefetching code path fetches 1 file for revision 5 and while doing so,
also fetches 3 trees dealing with the tree manifest of the base revision 2.

  $ hg prefetch
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)

Prefetching with treemanifest.ondemandfetch=True should fall back to normal
fetch is the server doesn't support it.

  $ rm -rf $CACHEDIR/master
  $ hg prefetch --config treemanifest.ondemandfetch=True
  fetching tree 'subdir' ddb35f099a648a43a997aef53123bce309c794fd
  1 trees fetched over 0.00s

Running prefetch in the master repository should exit gracefully

  $ cd ../master
  $ hg prefetch
