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
  $ dd if=/dev/zero bs=1k count=256 > a-large-file 2> /dev/null
  $ hg addremove
  adding a-large-file
  $ hg commit -m "add a-large-file (as a normal file)"
  $ find .hg/largefiles
  .hg/largefiles
  $ cd ..
  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo
  initializing destination largefiles-repo

"lfconvert" adds 'largefiles' to .hg/requires.
  $ cat largefiles-repo/.hg/requires
  largefiles
  revlogv1
  fncache
  store
  dotencode

"lfconvert" includes a newline at the end of the standin files.
  $ cd largefiles-repo
  $ hg up
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  getting changed largefiles
  1 largefiles updated, 0 removed
  $ cat .hglf/a-large-file
  2e000fa7e85759c7f4c254d4d9c33ef481e459a7
  $ dd if=/dev/zero bs=1k count=1k > another-large-file 2> /dev/null
  $ hg add --lfsize=1 another-large-file
  $ hg commit -m "add another-large-file (should be a largefile)"
  $ cat .hglf/a-large-file .hglf/another-large-file
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
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate
  a-large-file
  another-large-file
  $ [ -d .hg/largefiles ] && echo fail || echo pass
  pass

Cleanup
  $ cd ..
  $ rm -rf bigfile-repo largefiles-repo normal-repo
