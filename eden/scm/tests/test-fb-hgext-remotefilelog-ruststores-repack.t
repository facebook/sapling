#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True

  $ echo x > x
  $ hg commit -qAm x
  $ echo y > y
  $ hg commit -qAm y

  $ findfilessorted .hg/store/packs
  .hg/store/packs/2d66e09c3bf8a000428af1630d978127182e496e.dataidx
  .hg/store/packs/2d66e09c3bf8a000428af1630d978127182e496e.datapack
  .hg/store/packs/65749040bf285c8867cb0d12bdae7cbcac022a55.dataidx
  .hg/store/packs/65749040bf285c8867cb0d12bdae7cbcac022a55.datapack
  .hg/store/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histidx
  .hg/store/packs/c3399b56e035f73c3295276ed098235a08a0ed8c.histpack
  .hg/store/packs/ed1aaa9bfbf108367f595bdff7a706b587e188bc.histidx
  .hg/store/packs/ed1aaa9bfbf108367f595bdff7a706b587e188bc.histpack
  .hg/store/packs/manifests/1921bd3d3d8442c6f92cf8363675e538c36d062b.dataidx
  .hg/store/packs/manifests/1921bd3d3d8442c6f92cf8363675e538c36d062b.datapack
  .hg/store/packs/manifests/2105dd350da61d1a4f08cacbb82949d855edf5bb.histidx
  .hg/store/packs/manifests/2105dd350da61d1a4f08cacbb82949d855edf5bb.histpack
  .hg/store/packs/manifests/2bf8539e08195f796c4ada99d894c92b6447b73e.dataidx
  .hg/store/packs/manifests/2bf8539e08195f796c4ada99d894c92b6447b73e.datapack
  .hg/store/packs/manifests/a890c983659e18f095538fb20f217db4e7bb129d.histidx
  .hg/store/packs/manifests/a890c983659e18f095538fb20f217db4e7bb129d.histpack

  $ hg repack --debug --traceback

  $ findfilessorted .hg/store/packs
  .hg/store/packs/102e9c722b8edc89ad9e5a488ad8e5347bc7e213.dataidx
  .hg/store/packs/102e9c722b8edc89ad9e5a488ad8e5347bc7e213.datapack
  .hg/store/packs/ed6d1e892f0715dc798b5e31f8b5a546f6dc357f.histidx
  .hg/store/packs/ed6d1e892f0715dc798b5e31f8b5a546f6dc357f.histpack
  .hg/store/packs/manifests/7041e644145f0031dca8f552159e2bb2e30a9d62.dataidx
  .hg/store/packs/manifests/7041e644145f0031dca8f552159e2bb2e30a9d62.datapack
  .hg/store/packs/manifests/ab796727d5271e973a5f03cf927e0bc877a0fb53.histidx
  .hg/store/packs/manifests/ab796727d5271e973a5f03cf927e0bc877a0fb53.histpack

# Verify that the data is still what we expect.
  $ hg up null
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg up -r tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat x
  x
  $ cat y
  y

Test that we can repack packs into indexedlog
  $ hg push -q -r tip --to master --create
  $ clearcache
  $ clone master shallow2
  fetching tree '' d80a4bdb312d799dffbbce4719a5e2ad7987058e, found via d34c38483be9
  1 trees fetched over 0.00s
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)

# Verify stuff normally goes to packs
  $ ls $CACHEDIR/master/packs | grep datapack
  102e9c722b8edc89ad9e5a488ad8e5347bc7e213.datapack
  $ cd shallow2
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True

# Verify repack turns packs into indexedlog
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=True
  $ hg repack
  $ ls_l $CACHEDIR/master/indexedlogdatastore/0
  -rw-rw-r--      25 index2-node
  -rw-rw-r--     108 log
  -rw-rw-r--      * meta (glob)
  $ ls $CACHEDIR/master/packs/ | grep datapack
  [1]

# Verify new fetches go to the indexedlog
  $ clearcache
  $ hg prefetch -r .
  1 trees fetched over * (glob)
  $ ls_l $CACHEDIR/master/indexedlogdatastore/0
  -rw-rw-r--      25 index2-node
  -rw-rw-r--     108 log
  -rw-rw-r--      * meta (glob)
  $ ls $CACHEDIR/master/packs/ | grep datapack
  [1]
  $ hg cat -r tip x
  x
