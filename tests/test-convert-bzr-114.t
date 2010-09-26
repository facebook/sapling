
  $ "$TESTDIR/hghave" bzr114 || exit 80
  $ . "$TESTDIR/bzr-definitions"

The file/directory replacement can only be reproduced on
bzr >= 1.4. Merge it back in test-convert-bzr-directories once
this version becomes mainstream.
replace file with dir

  $ mkdir test-replace-file-with-dir
  $ cd test-replace-file-with-dir
  $ bzr init -q source
  $ cd source
  $ echo d > d
  $ bzr add -q d
  $ bzr commit -q -m 'add d file'
  $ rm d
  $ mkdir d
  $ bzr add -q d
  $ bzr commit -q -m 'replace with d dir'
  $ echo a > d/a
  $ bzr add -q d/a
  $ bzr commit -q -m 'add d/a'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  2 add d file
  1 replace with d dir
  0 add d/a
  $ manifest source-hg tip
  % manifest of tip
  644   d/a
  $ cd source-hg
  $ hg update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../..
