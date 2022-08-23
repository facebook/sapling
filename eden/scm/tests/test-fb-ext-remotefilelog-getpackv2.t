#chg-compatible
#require mononoke
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
  $ hg push -r tip --to master --create --config paths.default=mononoke://$(mononoke_address)/master
  remote: adding changesets (?)
  remote: adding manifests (?)
  remote: adding file changes (?)
  pushing rev 79c51fb96423 to destination mononoke://$LOCALIP:$LOCAL_PORT/master bookmark master
  searching for changes
  exporting bookmark master

  $ cd ..

Shallow clone from full

  $ clone master shallow --noupdate
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > fetchpacks=True
  > getpackversion=2
  > EOF

  $ hg up -q tip
  fetching tree '' 05bd2758dd7a25912490d0633b8975bf52bfab06
  1 trees fetched over 0.00s
  fetching tree 'dir' 8a87e5128a9877c501d5a20c32dbd2103a54afad
  1 trees fetched over 0.00s

  $ hg debugfilerev -v
  79c51fb96423: y
   dir/y: bin=0 lnk=0 flag=0 size=2 copied='' chain=076f5e2225b3
    rawdata: 'y\n'

Now try prefetchchunksize option, and expect that two getpackv2 calls were made
  $ hg up null --debug
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 79c51fb96423, local: 79c51fb96423+, remote: 000000000000
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ rm -rf "$TESTTMP"/hgcache/*
  $ ls "$TESTTMP"/hgcache/
  ls: cannot access '$TESTTMP/hgcache/': $ENOENT$
  [2]
  $ hg up tip --config remotefilelog.prefetchchunksize=1 --debug
  resolving manifests
   branchmerge: False, force: True, partial: False
   ancestor: 000000000000, local: 000000000000+, remote: 79c51fb96423
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

