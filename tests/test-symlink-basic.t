  $ "$TESTDIR/hghave" symlink || exit 80

  $ hg init a
  $ cd a
  $ ln -s nothing dangling
  $ hg commit -m 'commit symlink without adding' dangling
  abort: dangling: file not tracked!
  $ hg add dangling
  $ hg commit -m 'add symlink'

  $ hg tip -v
  changeset:   0:cabd88b706fc
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       dangling
  description:
  add symlink
  
  
  $ hg manifest --debug
  2564acbe54bbbedfbf608479340b359f04597f80 644 @ dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> nothing

  $ rm dangling
  $ ln -s void dangling
  $ hg commit -m 'change symlink'
  $ $TESTDIR/readlink.py dangling
  dangling -> void


modifying link

  $ rm dangling
  $ ln -s empty dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> empty


reverting to rev 0:

  $ hg revert -r 0 -a
  reverting dangling
  $ $TESTDIR/readlink.py dangling
  dangling -> nothing


backups:

  $ $TESTDIR/readlink.py *.orig
  dangling.orig -> empty
  $ rm *.orig
  $ hg up -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

copies

  $ hg cp -v dangling dangling2
  copying dangling to dangling2
  $ hg st -Cmard
  A dangling2
    dangling
  $ $TESTDIR/readlink.py dangling dangling2
  dangling -> void
  dangling2 -> void


issue995

  $ hg up -C
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir dir
  $ ln -s dir dirlink
  $ hg ci -qAm 'add dirlink'
  $ mkdir newdir
  $ mv dir newdir/dir
  $ mv dirlink newdir/dirlink
  $ hg mv -A dirlink newdir/dirlink
