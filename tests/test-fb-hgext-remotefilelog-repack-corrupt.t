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
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/aee31534993a501858fb6dd96a065671922e7d51
  $TESTTMP/hgcache/master/11/f6ad8ec52a2984abaafd7c3b516503785c2072/filename
  $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/d04f7aab46ef99f56ff77b65f696c719a647fc22
  $TESTTMP/hgcache/master/95/cb0bfd2977c761298d9624e4b4d4c72a39974a/filename

  $ hg repack

  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/ff17bece75c60ad09be6588d27d3f6e0ed5dd400.dataidx
  $TESTTMP/hgcache/master/packs/ff17bece75c60ad09be6588d27d3f6e0ed5dd400.datapack
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
  $TESTTMP/hgcache/master/86/f7e437faa5a7fce15d1ddcb9eaeaea377667b8/a80d06849b333b8a3d5c445f8ba3142010dcdc9e
  $TESTTMP/hgcache/master/86/f7e437faa5a7fce15d1ddcb9eaeaea377667b8/filename
  $TESTTMP/hgcache/master/e9/d71f5ee7c92d6dc9e92ffdad17b8bd49418f98/861f64b3905609e79fdbcf098c2ba5546fbc0789
  $TESTTMP/hgcache/master/e9/d71f5ee7c92d6dc9e92ffdad17b8bd49418f98/filename
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/ff17bece75c60ad09be6588d27d3f6e0ed5dd400.dataidx
  $TESTTMP/hgcache/master/packs/ff17bece75c60ad09be6588d27d3f6e0ed5dd400.datapack
  $TESTTMP/hgcache/master/packs/repacklock

Truncate the historypack file in the middle of the filename length for "y"
  $ chmod +w $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $ python $TESTDIR/truncate.py --size 173 $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack

Repack
  $ hg repack
  $ findfilessorted $CACHEDIR/master
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histidx
  $TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440.histpack
  $TESTTMP/hgcache/master/packs/4033e474e920b8b5d47d0904080195064445cee8.dataidx
  $TESTTMP/hgcache/master/packs/4033e474e920b8b5d47d0904080195064445cee8.datapack
  $TESTTMP/hgcache/master/packs/8335018efaefd765d52fad1c07f44addc0371202.histidx
  $TESTTMP/hgcache/master/packs/8335018efaefd765d52fad1c07f44addc0371202.histpack
  $TESTTMP/hgcache/master/packs/repacklock

The history for y has to be refetched from the server.
  $ hg log -f y -T '{desc}\n'
  deleting corrupt pack '$TESTTMP/hgcache/master/packs/37db2caec222ca26824a52d6bdc778344e0d1440'
  xy2
  xy
  2 files fetched over 2 fetches - (2 misses, 0.00% hit ratio) over 0.00s

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
  abort: error downloading file contents:
  'connection closed early for filename n and node c972a0820002b32c6fec4b7ca47d3aecdad8e1c5'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  [255]

The local data and history for o is lost
  $ hg cat -q o
  detected corrupt pack '$TESTTMP/shallow/.hg/store/packs/f0a7036b83e36fd41dc1ea89cc67e6a01487f2cf' - ignoring it
  abort: error downloading file contents:
  'connection closed early for filename o and node fd94f81d01bf8c9d960bb57abdd4e8375309ae43'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over 0.00s
  [255]
  $ hg log -qf o
  detected corrupt pack '$TESTTMP/shallow/.hg/store/packs/822f755410ca9f664d7c706957b8391248327318' - ignoring it
  abort: error downloading file contents:
  'connection closed early for filename o and node fd94f81d01bf8c9d960bb57abdd4e8375309ae43'
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  [255]
