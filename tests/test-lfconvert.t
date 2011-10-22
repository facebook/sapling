  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles =
  > share =
  > [largefiles]
  > minsize = 0.5
  > patterns = **.dat
  > EOF

"lfconvert" works
  $ hg init bigfile-repo
  $ cd bigfile-repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > largefiles = !
  > EOF
  $ mkdir sub
  $ dd if=/dev/zero bs=1k count=256 > large 2> /dev/null
  $ echo normal > normal1
  $ echo alsonormal > sub/normal2
  $ dd if=/dev/zero bs=1k count=10 > sub/maybelarge.dat 2> /dev/null
  $ hg addremove
  adding large
  adding normal1
  adding sub/maybelarge.dat
  adding sub/normal2
  $ hg commit -m"add large, normal1" large normal1
  $ hg commit -m"add sub/*" sub
  $ [ -d .hg/largefiles ] && echo fail || echo pass
  pass
  $ cd ..
  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo
  initializing destination largefiles-repo

"lfconvert" converts content correctly
  $ cd largefiles-repo
  $ hg up
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  2 largefiles updated, 0 removed
  $ hg locate
  .hglf/large
  .hglf/sub/maybelarge.dat
  normal1
  sub/normal2
  $ cat normal1
  normal
  $ cat sub/normal2
  alsonormal
  $ sha1sum large sub/maybelarge.dat
  2e000fa7e85759c7f4c254d4d9c33ef481e459a7  large
  34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c  sub/maybelarge.dat

"lfconvert" adds 'largefiles' to .hg/requires.
  $ cat .hg/requires
  largefiles
  revlogv1
  fncache
  store
  dotencode

"lfconvert" includes a newline at the end of the standin files.
  $ cat .hglf/large .hglf/sub/maybelarge.dat
  2e000fa7e85759c7f4c254d4d9c33ef481e459a7
  34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c

add another largefile to the new largefiles repo
  $ dd if=/dev/zero bs=1k count=1k > anotherlarge 2> /dev/null
  $ hg add --lfsize=1 anotherlarge
  $ hg commit -m "add anotherlarge (should be a largefile)"
  $ cat .hglf/large .hglf/anotherlarge
  2e000fa7e85759c7f4c254d4d9c33ef481e459a7
  3b71f43ff30f4b15b5cd85dd9e95ebc7e84eb5a3
  $ cd ..

"lfconvert" error cases
  $ hg lfconvert http://localhost/foo foo
  abort: http://localhost/foo is not a local Mercurial repo
  [255]
  $ hg lfconvert foo ssh://localhost/foo
  abort: ssh://localhost/foo is not a local Mercurial repo
  [255]
  $ hg lfconvert nosuchrepo foo
  abort: repository nosuchrepo not found!
  [255]
  $ hg share -q -U bigfile-repo shared
  $ echo -n bogus > shared/.hg/sharedpath
  $ hg lfconvert shared foo
  abort: .hg/sharedpath points to nonexistent directory $TESTTMP/bogus!
  [255]
  $ hg lfconvert bigfile-repo largefiles-repo
  initializing destination largefiles-repo
  abort: repository largefiles-repo already exists!
  [255]

Convert back to a normal (non-largefiles) repo
  $ cd largefiles-repo
  $ hg lfconvert --to-normal . ../normal-repo
  initializing destination ../normal-repo
  $ cd ../normal-repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > largefiles = !
  > EOF
  $ hg update
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate
  anotherlarge
  large
  normal1
  sub/maybelarge.dat
  sub/normal2
  $ [ -d .hg/largefiles ] && echo fail || echo pass
  pass
