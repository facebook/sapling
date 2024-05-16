#chg-compatible
#debugruntest-incompatible

  $ configure modernclient

  $ tipparents() {
  > hg parents --template "{node|short} {desc|firstline}\n" -r .
  > }

Test import and merge diffs

  $ newclientrepo repo test:server
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg ci -m changea
  $ echo c > c
  $ hg ci -Am addc
  adding c
  $ hg push -r . -q --to rev2 --create
  $ hg up 'desc(adda)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg push -r . -q --to rev3 --create
  $ hg up 'desc(changea)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc(addb)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ hg export . > ../merge.diff
  $ grep -v '^merge$' ../merge.diff > ../merge.nomsg.diff
  $ newclientrepo repo2 test:server rev2
  $ hg pull -B rev3
  pulling from test:server
  searching for changes

Test without --exact and diff.p1 == workingdir.p1

  $ hg up 'desc(changea)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat > $TESTTMP/editor.sh <<EOF
  > env | grep HGEDITFORM
  > echo merge > \$1
  > EOF
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg import --edit ../merge.nomsg.diff
  applying ../merge.nomsg.diff
  HGEDITFORM=import.normal.merge
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg hide -q -r .

Test without --exact and diff.p1 != workingdir.p1

  $ hg up 'desc(addc)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import ../merge.diff
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ tipparents
  890ecaa90481 addc
  $ hg hide -q -r .

Test with --exact

  $ hg import --exact ../merge.diff
  applying ../merge.diff
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg hide -q -r .

Test with --bypass and diff.p1 == workingdir.p1

  $ hg up 'desc(changea)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff -m 'merge-bypass1'
  applying ../merge.diff
  $ hg up -q 'desc("merge-bypass1")'
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg hide -q -r .

Test with --bypass and diff.p1 != workingdir.p1

  $ hg up 'desc(addc)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff -m 'merge-bypass2'
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ hg up -q 'desc("merge-bypass2")'
  $ tipparents
  890ecaa90481 addc
  $ hg hide -q -r .

  $ cd ..

Test that --exact on a bad header doesn't corrupt the repo (issue3616)

  $ newclientrepo repo3
  $ echo a>a
  $ hg ci -Aqm0
  $ hg push -q -r . --to rev0 --create
  $ echo a>>a
  $ hg ci -m1
  $ hg push -q -r . --to rev1 --create
  $ echo a>>a
  $ hg ci -m2
  $ echo a>a
  $ echo b>>a
  $ echo a>>a
  $ hg ci -m3
  $ hg export 'desc(2)' > $TESTTMP/p
  $ head -7 $TESTTMP/p > ../a.patch
  $ hg export tip > out
  >>> apatch = open("../a.patch", "ab")
  >>> _ = apatch.write(b"".join(open("out", "rb").readlines()[7:]))

  $ newclientrepo repor-clone test:repo3_server rev0
  $ hg pull -q -B rev1

  $ hg import --exact ../a.patch
  applying ../a.patch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  patching file a
  Hunk #1 succeeded at 1 with fuzz 1 (offset -1 lines).
  abort: patch is damaged or loses information
  [255]
