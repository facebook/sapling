#require hardlink

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "relink=" >> $HGRCPATH

  $ fix_path() {
  >     tr '\\' /
  > }

  $ cat > arelinked.py <<EOF
  > import sys, os
  > from mercurial import util
  > path1, path2 = sys.argv[1:3]
  > if util.samefile(path1, path2):
  >     print '%s == %s' % (path1, path2)
  > else:
  >     print '%s != %s' % (path1, path2)
  > EOF


create source repository

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ echo b > b
  $ hg ci -Am addfile
  adding a
  adding b
  $ cat "$TESTDIR/binfile.bin" >> a
  $ cat "$TESTDIR/binfile.bin" >> b
  $ hg ci -Am changefiles

make another commit to create files larger than 1 KB to test
formatting of final byte count

  $ cat "$TESTDIR/binfile.bin" >> a
  $ cat "$TESTDIR/binfile.bin" >> b
  $ hg ci -m anotherchange

don't sit forever trying to double-lock the source repo

  $ hg relink .
  relinking $TESTTMP/repo/.hg/store to $TESTTMP/repo/.hg/store (glob)
  there is nothing to relink


Test files are read in binary mode

  $ $PYTHON -c "file('.hg/store/data/dummy.i', 'wb').write('a\r\nb\n')"
  $ cd ..


clone and pull to break links

  $ hg clone --pull -r0 repo clone
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd clone
  $ hg pull -q
  $ echo b >> b
  $ hg ci -m changeb
  created new head
  $ $PYTHON -c "file('.hg/store/data/dummy.i', 'wb').write('a\nb\r\n')"


relink

  $ hg relink --debug --config progress.debug=true | fix_path
  relinking $TESTTMP/repo/.hg/store to $TESTTMP/clone/.hg/store
  tip has 2 files, estimated total number of files: 3
  collecting: 00changelog.i 1/3 files (33.33%)
  collecting: 00manifest.i 2/3 files (66.67%)
  collecting: a.i 3/3 files (100.00%)
  collecting: b.i 4/3 files (133.33%)
  collecting: dummy.i 5/3 files (166.67%)
  collected 5 candidate storage files
  not linkable: 00changelog.i
  not linkable: 00manifest.i
  pruning: data/a.i 3/5 files (60.00%)
  not linkable: data/b.i
  pruning: data/dummy.i 5/5 files (100.00%)
  pruned down to 2 probably relinkable files
  relinking: data/a.i 1/2 files (50.00%)
  not linkable: data/dummy.i
  relinked 1 files (1.36 KB reclaimed)
  $ cd ..


check hardlinks

  $ python arelinked.py repo/.hg/store/data/a.i clone/.hg/store/data/a.i
  repo/.hg/store/data/a.i == clone/.hg/store/data/a.i
  $ python arelinked.py repo/.hg/store/data/b.i clone/.hg/store/data/b.i
  repo/.hg/store/data/b.i != clone/.hg/store/data/b.i

