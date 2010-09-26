
  $ . "$TESTDIR/bzr-definitions"

create and rename on the same file in the same step

  $ mkdir test-createandrename
  $ cd test-createandrename
  $ bzr init -q source
  $ cd source
  $ echo a > a
  $ echo c > c
  $ echo e > e
  $ bzr add -q a c e
  $ bzr commit -q -m 'Initial add: a, c, e'
  $ bzr mv a b
  a => b
  $ bzr mv c d
  c => d
  $ bzr mv e f
  e => f
  $ echo a2 >> a
  $ mkdir e
  $ bzr add -q a e
  $ bzr commit -q -m 'rename a into b, create a, rename c into d'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial add: a, c, e
  0 rename a into b, create a, rename c into d
  $ glog -R source-hg
  o  1 "rename a into b, create a, rename c into d" files: a b c d e f
  |
  o  0 "Initial add: a, c, e" files: a c e
  

manifest

  $ hg manifest -R source-hg -r tip
  a
  b
  d
  f

test --rev option

  $ hg convert -r 1 source source-1-hg
  initializing destination source-1-hg repository
  scanning source...
  sorting...
  converting...
  0 Initial add: a, c, e
  $ glog -R source-1-hg
  o  0 "Initial add: a, c, e" files: a c e
  

test with filemap

  $ cat > filemap <<EOF
  > exclude a
  > EOF
  $ hg convert --filemap filemap source source-filemap-hg
  initializing destination source-filemap-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial add: a, c, e
  0 rename a into b, create a, rename c into d
  $ hg -R source-filemap-hg manifest -r tip
  b
  d
  f

convert from lightweight checkout

  $ bzr checkout --lightweight source source-light
  $ hg convert source-light source-light-hg
  initializing destination source-light-hg repository
  warning: lightweight checkouts may cause conversion failures, try with a regular branch instead.
  scanning source...
  sorting...
  converting...
  1 Initial add: a, c, e
  0 rename a into b, create a, rename c into d

lightweight manifest

  $ hg manifest -R source-light-hg -r tip
  a
  b
  d
  f

extract timestamps that look just like hg's {date|isodate}:
yyyy-mm-dd HH:MM zzzz (no seconds!)
compare timestamps

  $ cd source
  $ bzr log | \
  >   sed '/timestamp/!d;s/.\{15\}\([0-9: -]\{16\}\):.. \(.[0-9]\{4\}\)/\1 \2/' \
  >   > ../bzr-timestamps
  $ cd ..
  $ hg -R source-hg log --template "{date|isodate}\n" > hg-timestamps
  $ diff -u bzr-timestamps hg-timestamps
  $ cd ..

merge

  $ mkdir test-merge
  $ cd test-merge
  $ cat > helper.py <<EOF
  > import sys
  > from bzrlib import workingtree
  > wt = workingtree.WorkingTree.open('.')
  > 
  > message, stamp = sys.argv[1:]
  > wt.commit(message, timestamp=int(stamp))
  > EOF
  $ bzr init -q source
  $ cd source
  $ echo content > a
  $ echo content2 > b
  $ bzr add -q a b
  $ bzr commit -q -m 'Initial add'
  $ cd ..
  $ bzr branch -q source source-improve
  $ cd source
  $ echo more >> a
  $ python ../helper.py 'Editing a' 100
  $ cd ../source-improve
  $ echo content3 >> b
  $ python ../helper.py 'Editing b' 200
  $ cd ../source
  $ bzr merge -q ../source-improve
  $ bzr commit -q -m 'Merged improve branch'
  $ cd ..
  $ hg convert --datesort source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  3 Initial add
  2 Editing a
  1 Editing b
  0 Merged improve branch
  $ glog -R source-hg
  o    3 "Merged improve branch" files:
  |\
  | o  2 "Editing b" files: b
  | |
  o |  1 "Editing a" files: a
  |/
  o  0 "Initial add" files: a b
  
  $ cd ..

symlinks and executable files

  $ mkdir test-symlinks
  $ cd test-symlinks
  $ bzr init -q source
  $ cd source
  $ touch program
  $ chmod +x program
  $ ln -s program altname
  $ mkdir d
  $ echo a > d/a
  $ ln -s a syma
  $ bzr add -q altname program syma d/a
  $ bzr commit -q -m 'Initial setup'
  $ touch newprog
  $ chmod +x newprog
  $ rm altname
  $ ln -s newprog altname
  $ chmod -x program
  $ bzr add -q newprog
  $ bzr commit -q -m 'Symlink changed, x bits changed'
  $ cd ..
  $ hg convert source source-hg
  initializing destination source-hg repository
  scanning source...
  sorting...
  converting...
  1 Initial setup
  0 Symlink changed, x bits changed
  $ manifest source-hg 0
  % manifest of 0
  644 @ altname
  644   d/a
  755 * program
  644 @ syma
  $ manifest source-hg tip
  % manifest of tip
  644 @ altname
  644   d/a
  755 * newprog
  644   program
  644 @ syma
  $ cd source-hg

test the symlinks can be recreated

  $ hg up
  5 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg cat syma; echo
  a

