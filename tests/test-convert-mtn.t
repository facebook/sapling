
  $ "$TESTDIR/hghave" mtn || exit 80

Monotone directory is called .monotone on *nix and monotone
on Windows. Having a variable here ease test patching.

  $ mtndir=.monotone
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ echo 'graphlog =' >> $HGRCPATH
  $ HOME=`pwd`/do_not_use_HOME_mtn; export HOME

Windows version of monotone home

  $ APPDATA=$HOME; export APPDATA

tedious monotone keys configuration
The /dev/null redirection is necessary under Windows, or
it complains about home directory permissions

  $ mtn --quiet genkey test@selenic.com 1>/dev/null 2>&1 <<EOF
  > passphrase
  > passphrase
  > EOF
  $ cat >> $HOME/$mtndir/monotonerc <<EOF
  > function get_passphrase(keypair_id)
  >     return "passphrase"
  > end
  > EOF

create monotone repository

  $ mtn db init --db=repo.mtn
  $ mtn --db=repo.mtn --branch=com.selenic.test setup workingdir
  $ cd workingdir
  $ echo a > a
  $ mkdir dir
  $ echo b > dir/b
  $ echo d > dir/d
  $ python -c 'file("bin", "wb").write("a\\x00b")'
  $ echo c > c
  $ mtn add a dir/b dir/d c bin
  mtn: adding a to workspace manifest
  mtn: adding bin to workspace manifest
  mtn: adding c to workspace manifest
  mtn: adding dir to workspace manifest
  mtn: adding dir/b to workspace manifest
  mtn: adding dir/d to workspace manifest
  $ mtn ci -m initialize
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 0f6e5e4f2e7d2a8ef312408f57618abf026afd90

update monotone working directory

  $ mtn mv a dir/a
  mtn: skipping dir, already accounted for in workspace
  mtn: renaming a to dir/a in workspace manifest
  $ echo a >> dir/a
  $ echo b >> dir/b
  $ mtn drop c
  mtn: dropping c from workspace manifest
  $ python -c 'file("bin", "wb").write("b\\x00c")'
  $ mtn ci -m update1
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 51d0a982464573a2a2cf5ee2c9219c652aaebeff
  $ cd ..

convert once

  $ hg convert -s mtn repo.mtn
  assuming destination repo.mtn-hg
  initializing destination repo.mtn-hg repository
  scanning source...
  sorting...
  converting...
  1 initialize
  0 update1
  $ cd workingdir
  $ echo e > e
  $ mtn add e
  mtn: adding e to workspace manifest
  $ mtn drop dir/b
  mtn: dropping dir/b from workspace manifest
  $ mtn mv bin bin2
  mtn: renaming bin to bin2 in workspace manifest
  $ mtn ci -m 'update2 "with" quotes'
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision ebe58335d85d8cb176b6d0a12be04f5314b998da

test directory move

  $ mkdir -p dir1/subdir1
  $ mkdir -p dir1/subdir2_other
  $ echo file1 > dir1/subdir1/file1
  $ echo file2 > dir1/subdir2_other/file1
  $ mtn add dir1/subdir1/file1 dir1/subdir2_other/file1
  mtn: adding dir1 to workspace manifest
  mtn: adding dir1/subdir1 to workspace manifest
  mtn: adding dir1/subdir1/file1 to workspace manifest
  mtn: adding dir1/subdir2_other to workspace manifest
  mtn: adding dir1/subdir2_other/file1 to workspace manifest
  $ mtn ci -m createdir1
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision a8d62bc04fee4d2936d28e98bbcc81686dd74306
  $ mtn rename dir1/subdir1 dir1/subdir2
  mtn: skipping dir1, already accounted for in workspace
  mtn: renaming dir1/subdir1 to dir1/subdir2 in workspace manifest
  $ mtn ci -m movedir1
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 2c3d241bbbfe538b1b51d910f5676407e3f4d3a6

test subdirectory move

  $ mtn mv dir dir2
  mtn: renaming dir to dir2 in workspace manifest
  $ echo newfile > dir2/newfile
  $ mtn drop dir2/d
  mtn: dropping dir2/d from workspace manifest
  $ mtn add dir2/newfile
  mtn: adding dir2/newfile to workspace manifest
  $ mtn ci -m movedir
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision fdb5a02dae8bfce3a79b3393680af471016e1b4c

Test directory removal with empty directory

  $ mkdir dir2/dir
  $ mkdir dir2/dir/subdir
  $ echo f > dir2/dir/subdir/f
  $ mkdir dir2/dir/emptydir
  $ mtn add --quiet -R dir2/dir
  $ mtn ci -m emptydir
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 8bbf76d717001d24964e4604739fdcd0f539fc88
  $ mtn drop -R dir2/dir
  mtn: dropping dir2/dir/subdir/f from workspace manifest
  mtn: dropping dir2/dir/subdir from workspace manifest
  mtn: dropping dir2/dir/emptydir from workspace manifest
  mtn: dropping dir2/dir from workspace manifest
  $ mtn ci -m dropdirectory
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 2323d4bc324e6c82628dc04d47a9fd32ad24e322

test directory and file move

  $ mkdir -p dir3/d1
  $ echo a > dir3/a
  $ mtn add dir3/a dir3/d1
  mtn: adding dir3 to workspace manifest
  mtn: adding dir3/a to workspace manifest
  mtn: adding dir3/d1 to workspace manifest
  $ mtn ci -m dirfilemove
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 47b192f720faa622f48c68d1eb075b26d405aa8b
  $ mtn mv dir3/a dir3/d1/a
  mtn: skipping dir3/d1, already accounted for in workspace
  mtn: renaming dir3/a to dir3/d1/a in workspace manifest
  $ mtn mv dir3/d1 dir3/d2
  mtn: skipping dir3, already accounted for in workspace
  mtn: renaming dir3/d1 to dir3/d2 in workspace manifest
  $ mtn ci -m dirfilemove2
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 8b543a400d3ee7f6d4bb1835b9b9e3747c8cb632

test directory move into another directory move

  $ mkdir dir4
  $ mkdir dir5
  $ echo a > dir4/a
  $ mtn add dir4/a dir5
  mtn: adding dir4 to workspace manifest
  mtn: adding dir4/a to workspace manifest
  mtn: adding dir5 to workspace manifest
  $ mtn ci -m dirdirmove
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 466e0b2afc7a55aa2b4ab2f57cb240bb6cd66fc7
  $ mtn mv dir5 dir6
  mtn: renaming dir5 to dir6 in workspace manifest
  $ mtn mv dir4 dir6/dir4
  mtn: skipping dir6, already accounted for in workspace
  mtn: renaming dir4 to dir6/dir4 in workspace manifest
  $ mtn ci -m dirdirmove2
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 3d1f77ebad0c23a5d14911be3b670f990991b749

test diverging directory moves

  $ mkdir -p dir7/dir9/dir8
  $ echo a > dir7/dir9/dir8/a
  $ echo b > dir7/dir9/b
  $ echo c > dir7/c
  $ mtn add -R dir7
  mtn: adding dir7 to workspace manifest
  mtn: adding dir7/c to workspace manifest
  mtn: adding dir7/dir9 to workspace manifest
  mtn: adding dir7/dir9/b to workspace manifest
  mtn: adding dir7/dir9/dir8 to workspace manifest
  mtn: adding dir7/dir9/dir8/a to workspace manifest
  $ mtn ci -m divergentdirmove
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 08a08511f18b428d840199b062de90d0396bc2ed
  $ mtn mv dir7 dir7-2
  mtn: renaming dir7 to dir7-2 in workspace manifest
  $ mtn mv dir7-2/dir9 dir9-2
  mtn: renaming dir7-2/dir9 to dir9-2 in workspace manifest
  $ mtn mv dir9-2/dir8 dir8-2
  mtn: renaming dir9-2/dir8 to dir8-2 in workspace manifest
  $ mtn ci -m divergentdirmove2
  mtn: beginning commit on branch 'com.selenic.test'
  mtn: committed revision 4a736634505795f17786fffdf2c9cbf5b11df6f6
  $ cd ..

convert incrementally

  $ hg convert -s mtn repo.mtn
  assuming destination repo.mtn-hg
  scanning source...
  sorting...
  converting...
  11 update2 "with" quotes
  10 createdir1
  9 movedir1
  8 movedir
  7 emptydir
  6 dropdirectory
  5 dirfilemove
  4 dirfilemove2
  3 dirdirmove
  2 dirdirmove2
  1 divergentdirmove
  0 divergentdirmove2
  $ glog()
  > {
  >     hg glog --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
  > }
  $ cd repo.mtn-hg
  $ hg up -C
  11 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ glog
  @  13 "divergentdirmove2" files: dir7-2/c dir7/c dir7/dir9/b dir7/dir9/dir8/a dir8-2/a dir9-2/b
  |
  o  12 "divergentdirmove" files: dir7/c dir7/dir9/b dir7/dir9/dir8/a
  |
  o  11 "dirdirmove2" files: dir4/a dir6/dir4/a
  |
  o  10 "dirdirmove" files: dir4/a
  |
  o  9 "dirfilemove2" files: dir3/a dir3/d2/a
  |
  o  8 "dirfilemove" files: dir3/a
  |
  o  7 "dropdirectory" files: dir2/dir/subdir/f
  |
  o  6 "emptydir" files: dir2/dir/subdir/f
  |
  o  5 "movedir" files: dir/a dir/d dir2/a dir2/newfile
  |
  o  4 "movedir1" files: dir1/subdir1/file1 dir1/subdir2/file1
  |
  o  3 "createdir1" files: dir1/subdir1/file1 dir1/subdir2_other/file1
  |
  o  2 "update2 "with" quotes" files: bin bin2 dir/b e
  |
  o  1 "update1" files: a bin c dir/a dir/b
  |
  o  0 "initialize" files: a bin c dir/b dir/d
  

manifest

  $ hg manifest
  bin2
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  dir2/a
  dir2/newfile
  dir3/d2/a
  dir6/dir4/a
  dir7-2/c
  dir8-2/a
  dir9-2/b
  e

contents

  $ cat dir2/a
  a
  a
  $ test -d dir2/dir && echo 'removed dir2/dir is still there!'
  [1]

file move

  $ hg log -v -C -r 1 | grep copies
  copies:      dir/a (a)

check directory move

  $ hg manifest -r 4
  bin2
  dir/a
  dir/d
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  e
  $ test -d dir1/subdir2 || echo 'new dir1/subdir2 does not exist!'
  $ test -d dir1/subdir1 && echo 'renamed dir1/subdir1 is still there!'
  [1]
  $ hg log -v -C -r 4 | grep copies
  copies:      dir1/subdir2/file1 (dir1/subdir1/file1)

check file remove with directory move

  $ hg manifest -r 5
  bin2
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  dir2/a
  dir2/newfile
  e

check file move with directory move

  $ hg manifest -r 9
  bin2
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  dir2/a
  dir2/newfile
  dir3/d2/a
  e

check file directory directory move

  $ hg manifest -r 11
  bin2
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  dir2/a
  dir2/newfile
  dir3/d2/a
  dir6/dir4/a
  e

check divergent directory moves

  $ hg manifest -r 13
  bin2
  dir1/subdir2/file1
  dir1/subdir2_other/file1
  dir2/a
  dir2/newfile
  dir3/d2/a
  dir6/dir4/a
  dir7-2/c
  dir8-2/a
  dir9-2/b
  e
