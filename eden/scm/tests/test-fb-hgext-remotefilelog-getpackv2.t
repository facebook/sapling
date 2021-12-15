#chg-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1
  $ echo x > x
  $ hg commit -qAm x
  $ mkdir dir
  $ echo y > dir/y
  $ hg commit -qAm y
  $ hg push -r tip --to master --create
  pushing rev 79c51fb96423 to destination ssh://user@dummy/master bookmark master
  searching for changes
  exporting bookmark master
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  $ cd ..

Shallow clone from full

  $ clone master shallow --noupdate
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=2
  > EOF

#if mononoke
  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06
  1 trees fetched over 0.00s
  fetching tree 'dir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
  1 trees fetched over 0.00s
#else
  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over *s (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'dir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
  1 trees fetched over 0.00s
#endif

  $ hg debugfilerev -v
  79c51fb96423: y
   dir/y: bin=0 lnk=0 flag=0 size=2 copied='' chain=076f5e2225b3
    rawdata: 'y\n'

Now try prefetchchunksize option, and expect that two getpackv2 calls were made
  $ hg up -q null
  $ rm -r "$TESTTMP"/hgcache/*
  $ ls "$TESTTMP"/hgcache/
  $ hg up tip --config remotefilelog.prefetchchunksize=1 --debug 2>&1 | grep 'sending getpackv2 command'
  sending getpackv2 command
  sending getpackv2 command
