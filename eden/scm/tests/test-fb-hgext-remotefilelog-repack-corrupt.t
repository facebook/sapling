  $ setconfig extensions.treemanifest=!

  $ . "$TESTDIR/library.sh"

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > server=True
  > serverexpiration=-1
  > EOF
  $ echo x > x
  $ echo y > y
  $ hg commit -qAm xy
  $ echo x >> x
  $ echo y >> y
  $ hg commit -qAm xy2
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)

Set the prefetchdays config to zero so that all commits are prefetched
no matter what their creation date is.
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > localdatarepack=True
  > prefetchdays=0
  > EOF
  $ cd ..

  $ cd shallow
  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.dataidx
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.datapack

  $ hg repack

  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.dataidx
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.datapack
  $TESTTMP/hgcache/master/packs/repacklock

Create some new data to pack into it

  $ cd ../master
  $ echo a > a
  $ echo b > b
  $ hg commit -qAm ab
  $ echo a >> a
  $ echo b >> b
  $ hg commit -qAm ab2
  $ cd ../shallow
  $ hg pull -q
  $ hg up -q tip
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)
  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/767371d087d35f549346611a68c10805fa2e5083.dataidx
  $TESTTMP/hgcache/master/packs/767371d087d35f549346611a68c10805fa2e5083.datapack
  $TESTTMP/hgcache/master/packs/8054aa368dd33acb92013013096b517dfdfaf184.histidx
  $TESTTMP/hgcache/master/packs/8054aa368dd33acb92013013096b517dfdfaf184.histpack
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.dataidx
  $TESTTMP/hgcache/master/packs/9ae82bbfd147f28ff04367cf066c5ed3ef429be4.datapack
  $TESTTMP/hgcache/master/packs/repacklock

Truncate the historypack file in the middle of the filename length for "y"
  $ chmod +w $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $ python $TESTDIR/truncate.py --size 173 $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack

Repack
  $ hg repack
  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/60374dd52300114836388912c35fe94f80e74889.dataidx
  $TESTTMP/hgcache/master/packs/60374dd52300114836388912c35fe94f80e74889.datapack
  $TESTTMP/hgcache/master/packs/8335018efaefd765d52fad1c07f44addc0371202.histidx
  $TESTTMP/hgcache/master/packs/8335018efaefd765d52fad1c07f44addc0371202.histpack
  $TESTTMP/hgcache/master/packs/repacklock

The history for y has to be refetched from the server.
  $ hg log -f y -T '{desc}\n'
  deleting corrupt pack '$TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440'
  xy2
  xy
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s

Next, do the same for local data.  This time there is data loss, as there are no more copies
of the data available.

Create some local commits and pack them into a pack file

  $ echo m > m
  $ echo n > n
  $ echo o > o
  $ hg commit -qAm mno
  $ echo m >> m
  $ echo n >> n
  $ echo o >> o
  $ hg commit -qAm mno2
  $ hg repack

Truncate the history in the middle of the filename length for "n"
  $ chmod +w .hg/store/packs/822f755410ca9f664d7c706957b8391248327318.histpack
  $ python $TESTDIR/truncate.py --size 173 .hg/store/packs/822f755410ca9f664d7c706957b8391248327318.histpack

Truncate the data in the middle of the filename length for "o"
  $ chmod +w .hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf.datapack
  $ python $TESTDIR/truncate.py --size 130 .hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf.datapack

Repack
  $ hg repack
  $ findfilessorted .hg/store/packs
  .hg/store/packs/822f755410ca9f664d7c706957b8391248327318.histidx
  .hg/store/packs/822f755410ca9f664d7c706957b8391248327318.histpack
  .hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf.dataidx
  .hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf.datapack

The local data and history for m is still available
  $ hg cat m
  m
  m
  $ hg log -f m -T '{desc}\n'
  mno2
  mno

The local data for n is still available
  $ hg cat n
  n
  n

The history for n is lost
  $ hg log -qf n
  detected corrupt pack '$TESTTMP/shallow/.hg/store/packs/822f755410ca9f664d7c706957b8391248327318' - ignoring it
  abort: stream ended unexpectedly (got 0 bytes, expected 2)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  [255]

The local data and history for o is lost
  $ hg cat -q o
  detected corrupt pack '$TESTTMP/shallow/.hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf' - ignoring it
  abort: stream ended unexpectedly (got 0 bytes, expected 2)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  [255]
  $ hg log -qf o
  detected corrupt pack '$TESTTMP/shallow/.hg/store/packs/822f755410ca9f664d7c706957b8391248327318' - ignoring it
  abort: stream ended unexpectedly (got 0 bytes, expected 2)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  [255]
