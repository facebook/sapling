
  $ . "$TESTDIR/bzr-definitions"

empty directory

  $ mkdir test-empty
  $ cd test-empty
  $ bzr init -q source
  $ cd source
  $ echo content > a
  $ bzr add -q a
  $ bzr commit -q -m 'Initial add'
  $ mkdir empty
  $ bzr add -q empty
  $ bzr commit -q -m 'Empty directory added'
  $ echo content > empty/something
  $ bzr add -q empty/something
  $ bzr commit -q -m 'Added file into directory'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  2 Initial add
  1 Empty directory added
  0 Added file into directory
  $ manifest source-hg 1
  % manifest of 1
  644   a
  $ manifest source-hg tip
  % manifest of tip
  644   a
  644   empty/something
  $ cd ..

directory renames

  $ mkdir test-dir-rename
  $ cd test-dir-rename
  $ bzr init -q source
  $ cd source
  $ mkdir tpyo
  $ echo content > tpyo/something
  $ bzr add -q tpyo
  $ bzr commit -q -m 'Added directory'
  $ bzr mv tpyo typo
  tpyo => typo
  $ bzr commit -q -m 'Oops, typo'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Added directory
  0 Oops, typo
  $ manifest source-hg 0
  % manifest of 0
  644   tpyo/something
  $ manifest source-hg tip
  % manifest of tip
  644   typo/something
  $ cd ..

nested directory renames

  $ mkdir test-nested-dir-rename
  $ cd test-nested-dir-rename
  $ bzr init -q source
  $ cd source
  $ mkdir -p firstlevel/secondlevel/thirdlevel
  $ echo content > firstlevel/secondlevel/file
  $ echo this_needs_to_be_there_too > firstlevel/secondlevel/thirdlevel/stuff
  $ bzr add -q firstlevel
  $ bzr commit -q -m 'Added nested directories'
  $ bzr mv firstlevel/secondlevel secondlevel
  firstlevel/secondlevel => secondlevel
  $ bzr commit -q -m 'Moved secondlevel one level up'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Added nested directories
  0 Moved secondlevel one level up
  $ manifest source-hg tip
  % manifest of tip
  644   secondlevel/file
  644   secondlevel/thirdlevel/stuff
  $ cd ..

directory remove

  $ mkdir test-dir-remove
  $ cd test-dir-remove
  $ bzr init -q source
  $ cd source
  $ mkdir src
  $ echo content > src/sourcecode
  $ bzr add -q src
  $ bzr commit -q -m 'Added directory'
  $ bzr rm -q src
  $ bzr commit -q -m 'Removed directory'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Added directory
  0 Removed directory
  $ manifest source-hg 0
  % manifest of 0
  644   src/sourcecode
  $ manifest source-hg tip
  % manifest of tip
  $ cd ..

directory replace

  $ mkdir test-dir-replace
  $ cd test-dir-replace
  $ bzr init -q source
  $ cd source
  $ mkdir first second
  $ echo content > first/file
  $ echo morecontent > first/dummy
  $ echo othercontent > second/something
  $ bzr add -q first second
  $ bzr commit -q -m 'Initial layout'
  $ bzr mv first/file second/file
  first/file => second/file
  $ bzr mv first third
  first => third
  $ bzr commit -q -m 'Some conflicting moves'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial layout
  0 Some conflicting moves
  $ manifest source-hg tip
  % manifest of tip
  644   second/file
  644   second/something
  644   third/dummy
  $ cd ..
