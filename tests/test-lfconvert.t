  $ USERCACHE="$TESTTMP/cache"; export USERCACHE
  $ mkdir "${USERCACHE}"
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > largefiles =
  > share =
  > strip =
  > convert =
  > [largefiles]
  > minsize = 0.5
  > patterns = **.other
  >     **.dat
  > usercache=${USERCACHE}
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
  $ dd if=/dev/zero bs=1k count=256 > large2 2> /dev/null
  $ echo normal > normal1
  $ echo alsonormal > sub/normal2
  $ dd if=/dev/zero bs=1k count=10 > sub/maybelarge.dat 2> /dev/null
  $ hg addremove
  adding large
  adding large2
  adding normal1
  adding sub/maybelarge.dat
  adding sub/normal2
  $ hg commit -m"add large, normal1" large normal1
  $ hg commit -m"add sub/*" sub

Test tag parsing
  $ cat >> .hgtags <<EOF
  > IncorrectlyFormattedTag!
  > invalidhash sometag
  > 0123456789abcdef anothertag
  > EOF
  $ hg add .hgtags
  $ hg commit -m"add large2" large2 .hgtags

Test link+rename largefile codepath
  $ [ -d .hg/largefiles ] && echo fail || echo pass
  pass
  $ cd ..
  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo
  initializing destination largefiles-repo
  skipping incorrectly formatted tag IncorrectlyFormattedTag!
  skipping incorrectly formatted id invalidhash
  no mapping for id 0123456789abcdef
#if symlink
  $ hg --cwd bigfile-repo rename large2 large3
  $ ln -sf large bigfile-repo/large3
  $ hg --cwd bigfile-repo commit -m"make large2 a symlink" large2 large3
  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo-symlink
  initializing destination largefiles-repo-symlink
  skipping incorrectly formatted tag IncorrectlyFormattedTag!
  skipping incorrectly formatted id invalidhash
  no mapping for id 0123456789abcdef
  abort: renamed/copied largefile large3 becomes symlink
  [255]
#endif
  $ cd bigfile-repo
  $ hg strip --no-backup 2
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ cd ..
  $ rm -rf largefiles-repo largefiles-repo-symlink

  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo
  initializing destination largefiles-repo

"lfconvert" converts content correctly
  $ cd largefiles-repo
  $ hg up
  getting changed largefiles
  2 largefiles updated, 0 removed
  4 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate
  .hglf/large
  .hglf/sub/maybelarge.dat
  normal1
  sub/normal2
  $ cat normal1
  normal
  $ cat sub/normal2
  alsonormal
  $ "$TESTDIR/md5sum.py" large sub/maybelarge.dat
  ec87a838931d4d5d2e94a04644788a55  large
  1276481102f218c981e0324180bafd9f  sub/maybelarge.dat

"lfconvert" adds 'largefiles' to .hg/requires.
  $ cat .hg/requires
  dotencode
  fncache
  largefiles
  revlogv1
  store

"lfconvert" includes a newline at the end of the standin files.
  $ cat .hglf/large .hglf/sub/maybelarge.dat
  2e000fa7e85759c7f4c254d4d9c33ef481e459a7
  34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c
  $ cd ..

add some changesets to rename/remove/merge
  $ cd bigfile-repo
  $ hg mv -q sub stuff
  $ hg commit -m"rename sub/ to stuff/"
  $ hg update -q 1
  $ echo blah >> normal3
  $ echo blah >> sub/normal2
  $ echo blah >> sub/maybelarge.dat
  $ "$TESTDIR/md5sum.py" sub/maybelarge.dat
  1dd0b99ff80e19cff409702a1d3f5e15  sub/maybelarge.dat
  $ hg commit -A -m"add normal3, modify sub/*"
  adding normal3
  created new head
  $ hg rm large normal3
  $ hg commit -q -m"remove large, normal3"
  $ hg merge
  merging sub/maybelarge.dat and stuff/maybelarge.dat to stuff/maybelarge.dat
  warning: $TESTTMP/bigfile-repo/stuff/maybelarge.dat looks like a binary file. (glob)
  merging stuff/maybelarge.dat incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging sub/normal2 and stuff/normal2 to stuff/normal2
  0 files updated, 1 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg cat -r . sub/maybelarge.dat > stuff/maybelarge.dat
  $ hg resolve -m stuff/maybelarge.dat
  (no more unresolved files)
  $ hg commit -m"merge"
  $ hg log -G --template "{rev}:{node|short}  {desc|firstline}\n"
  @    5:4884f215abda  merge
  |\
  | o  4:7285f817b77e  remove large, normal3
  | |
  | o  3:67e3892e3534  add normal3, modify sub/*
  | |
  o |  2:c96c8beb5d56  rename sub/ to stuff/
  |/
  o  1:020c65d24e11  add sub/*
  |
  o  0:117b8328f97a  add large, normal1
  
  $ cd ..

lfconvert with rename, merge, and remove
  $ rm -rf largefiles-repo
  $ hg lfconvert --size 0.2 bigfile-repo largefiles-repo
  initializing destination largefiles-repo
  $ cd largefiles-repo
  $ hg log -G --template "{rev}:{node|short}  {desc|firstline}\n"
  o    5:8e05f5f2b77e  merge
  |\
  | o  4:a5a02de7a8e4  remove large, normal3
  | |
  | o  3:55759520c76f  add normal3, modify sub/*
  | |
  o |  2:261ad3f3f037  rename sub/ to stuff/
  |/
  o  1:334e5237836d  add sub/*
  |
  o  0:d4892ec57ce2  add large, normal1
  
  $ hg locate -r 2
  .hglf/large
  .hglf/stuff/maybelarge.dat
  normal1
  stuff/normal2
  $ hg locate -r 3
  .hglf/large
  .hglf/sub/maybelarge.dat
  normal1
  normal3
  sub/normal2
  $ hg locate -r 4
  .hglf/sub/maybelarge.dat
  normal1
  sub/normal2
  $ hg locate -r 5
  .hglf/stuff/maybelarge.dat
  normal1
  stuff/normal2
  $ hg update
  getting changed largefiles
  1 largefiles updated, 0 removed
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat stuff/normal2
  alsonormal
  blah
  $ "$TESTDIR/md5sum.py" stuff/maybelarge.dat
  1dd0b99ff80e19cff409702a1d3f5e15  stuff/maybelarge.dat
  $ cat .hglf/stuff/maybelarge.dat
  76236b6a2c6102826c61af4297dd738fb3b1de38
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
  $ printf 'bogus' > shared/.hg/sharedpath
  $ hg lfconvert shared foo
  abort: .hg/sharedpath points to nonexistent directory $TESTTMP/bogus! (glob)
  [255]
  $ hg lfconvert bigfile-repo largefiles-repo
  initializing destination largefiles-repo
  abort: repository largefiles-repo already exists!
  [255]

add another largefile to the new largefiles repo
  $ cd largefiles-repo
  $ dd if=/dev/zero bs=1k count=1k > anotherlarge 2> /dev/null
  $ hg add --lfsize=1 anotherlarge
  $ hg commit -m "add anotherlarge (should be a largefile)"
  $ cat .hglf/anotherlarge
  3b71f43ff30f4b15b5cd85dd9e95ebc7e84eb5a3
  $ hg tag mytag
  $ cd ..

round-trip: converting back to a normal (non-largefiles) repo with
"lfconvert --to-normal" should give the same as ../bigfile-repo
  $ cd largefiles-repo
  $ hg lfconvert --to-normal . ../normal-repo
  initializing destination ../normal-repo
  0 additional largefiles cached
  scanning source...
  sorting...
  converting...
  7 add large, normal1
  6 add sub/*
  5 rename sub/ to stuff/
  4 add normal3, modify sub/*
  3 remove large, normal3
  2 merge
  1 add anotherlarge (should be a largefile)
  0 Added tag mytag for changeset abacddda7028
  $ cd ../normal-repo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > largefiles = !
  > EOF

  $ hg log -G --template "{rev}:{node|short}  {desc|firstline}\n"
  o  7:b5fedc110b9d  Added tag mytag for changeset 867ab992ecf4
  |
  o  6:867ab992ecf4  add anotherlarge (should be a largefile)
  |
  o    5:4884f215abda  merge
  |\
  | o  4:7285f817b77e  remove large, normal3
  | |
  | o  3:67e3892e3534  add normal3, modify sub/*
  | |
  o |  2:c96c8beb5d56  rename sub/ to stuff/
  |/
  o  1:020c65d24e11  add sub/*
  |
  o  0:117b8328f97a  add large, normal1
  
  $ hg update
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg locate
  .hgtags
  anotherlarge
  normal1
  stuff/maybelarge.dat
  stuff/normal2
  $ [ -d .hg/largefiles ] && echo fail || echo pass
  pass

  $ cd ..

Clearing the usercache ensures that commitctx doesn't try to cache largefiles
from the working dir on a convert.
  $ rm "${USERCACHE}"/*
  $ hg convert largefiles-repo
  assuming destination largefiles-repo-hg
  initializing destination largefiles-repo-hg repository
  scanning source...
  sorting...
  converting...
  7 add large, normal1
  6 add sub/*
  5 rename sub/ to stuff/
  4 add normal3, modify sub/*
  3 remove large, normal3
  2 merge
  1 add anotherlarge (should be a largefile)
  0 Added tag mytag for changeset abacddda7028

  $ hg -R largefiles-repo-hg log -G --template "{rev}:{node|short}  {desc|firstline}\n"
  o  7:2f08f66459b7  Added tag mytag for changeset 17126745edfd
  |
  o  6:17126745edfd  add anotherlarge (should be a largefile)
  |
  o    5:9cc5aa7204f0  merge
  |\
  | o  4:a5a02de7a8e4  remove large, normal3
  | |
  | o  3:55759520c76f  add normal3, modify sub/*
  | |
  o |  2:261ad3f3f037  rename sub/ to stuff/
  |/
  o  1:334e5237836d  add sub/*
  |
  o  0:d4892ec57ce2  add large, normal1
  
Verify will fail (for now) if the usercache is purged before converting, since
largefiles are not cached in the converted repo's local store by the conversion
process.
  $ hg -R largefiles-repo-hg verify --large --lfa
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  9 files, 8 changesets, 13 total revisions
  searching 8 changesets for largefiles
  changeset 0:d4892ec57ce2: large references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/2e000fa7e85759c7f4c254d4d9c33ef481e459a7 (glob)
  changeset 1:334e5237836d: sub/maybelarge.dat references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c (glob)
  changeset 2:261ad3f3f037: stuff/maybelarge.dat references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c (glob)
  changeset 3:55759520c76f: sub/maybelarge.dat references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/76236b6a2c6102826c61af4297dd738fb3b1de38 (glob)
  changeset 5:9cc5aa7204f0: stuff/maybelarge.dat references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/76236b6a2c6102826c61af4297dd738fb3b1de38 (glob)
  changeset 6:17126745edfd: anotherlarge references missing $TESTTMP/largefiles-repo-hg/.hg/largefiles/3b71f43ff30f4b15b5cd85dd9e95ebc7e84eb5a3 (glob)
  verified existence of 6 revisions of 4 largefiles
  [1]
  $ hg -R largefiles-repo-hg showconfig paths
  [1]


Avoid a traceback if a largefile isn't available (issue3519)

Ensure the largefile can be cached in the source if necessary
  $ hg clone -U largefiles-repo issue3519
  $ rm -f "${USERCACHE}"/*
  $ hg lfconvert --to-normal issue3519 normalized3519
  initializing destination normalized3519
  4 additional largefiles cached
  scanning source...
  sorting...
  converting...
  7 add large, normal1
  6 add sub/*
  5 rename sub/ to stuff/
  4 add normal3, modify sub/*
  3 remove large, normal3
  2 merge
  1 add anotherlarge (should be a largefile)
  0 Added tag mytag for changeset abacddda7028

Ensure the abort message is useful if a largefile is entirely unavailable
  $ rm -rf normalized3519
  $ rm "${USERCACHE}"/*
  $ rm issue3519/.hg/largefiles/*
  $ rm largefiles-repo/.hg/largefiles/*
  $ hg lfconvert --to-normal issue3519 normalized3519
  initializing destination normalized3519
  anotherlarge: largefile 3b71f43ff30f4b15b5cd85dd9e95ebc7e84eb5a3 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  stuff/maybelarge.dat: largefile 76236b6a2c6102826c61af4297dd738fb3b1de38 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  stuff/maybelarge.dat: largefile 76236b6a2c6102826c61af4297dd738fb3b1de38 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  sub/maybelarge.dat: largefile 76236b6a2c6102826c61af4297dd738fb3b1de38 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  large: largefile 2e000fa7e85759c7f4c254d4d9c33ef481e459a7 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  sub/maybelarge.dat: largefile 76236b6a2c6102826c61af4297dd738fb3b1de38 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  large: largefile 2e000fa7e85759c7f4c254d4d9c33ef481e459a7 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  stuff/maybelarge.dat: largefile 34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c not available from file:/*/$TESTTMP/largefiles-repo (glob)
  large: largefile 2e000fa7e85759c7f4c254d4d9c33ef481e459a7 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  sub/maybelarge.dat: largefile 34e163be8e43c5631d8b92e9c43ab0bf0fa62b9c not available from file:/*/$TESTTMP/largefiles-repo (glob)
  large: largefile 2e000fa7e85759c7f4c254d4d9c33ef481e459a7 not available from file:/*/$TESTTMP/largefiles-repo (glob)
  0 additional largefiles cached
  11 largefiles failed to download
  abort: all largefiles must be present locally
  [255]


