  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ tipparents() {
  > hg parents --template "{rev}:{node|short} {desc|firstline}\n" -r tip
  > }

Test import and merge diffs

  $ hg init repo
  $ cd repo
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg ci -m changea
  $ echo c > c
  $ hg ci -Am addc
  adding c
  $ hg up 0
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am addb
  adding b
  created new head
  $ hg up 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge
  $ hg export . > ../merge.diff
  $ grep -v '^merge$' ../merge.diff > ../merge.nomsg.diff
  $ cd ..
  $ hg clone -r2 repo repo2
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo2
  $ hg pull -r3 ../repo
  pulling from ../repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  (run 'hg heads' to see heads, 'hg merge' to merge)

Test without --exact and diff.p1 == workingdir.p1

  $ hg up 1
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ cat > $TESTTMP/editor.sh <<EOF
  > env | grep HGEDITFORM
  > echo merge > \$1
  > EOF
  $ HGEDITOR="sh $TESTTMP/editor.sh" hg import --edit ../merge.nomsg.diff
  applying ../merge.nomsg.diff
  HGEDITFORM=import.normal.merge
  $ tipparents
  1:540395c44225 changea
  3:102a90ea7b4a addb
  $ hg strip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test without --exact and diff.p1 != workingdir.p1

  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import ../merge.diff
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ tipparents
  2:890ecaa90481 addc
  $ hg strip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test with --exact

  $ hg import --exact ../merge.diff
  applying ../merge.diff
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tipparents
  1:540395c44225 changea
  3:102a90ea7b4a addb
  $ hg strip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test with --bypass and diff.p1 == workingdir.p1

  $ hg up 1
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff
  applying ../merge.diff
  $ tipparents
  1:540395c44225 changea
  3:102a90ea7b4a addb
  $ hg strip --no-backup tip

Test with --bypass and diff.p1 != workingdir.p1

  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ tipparents
  2:890ecaa90481 addc
  $ hg strip --no-backup tip

Test with --bypass and --exact

  $ hg import --bypass --exact ../merge.diff
  applying ../merge.diff
  $ tipparents
  1:540395c44225 changea
  3:102a90ea7b4a addb
  $ hg strip --no-backup tip

  $ cd ..

Test that --exact on a bad header doesn't corrupt the repo (issue3616)

  $ hg init repo3
  $ cd repo3
  $ echo a>a
  $ hg ci -Aqm0
  $ echo a>>a
  $ hg ci -m1
  $ echo a>>a
  $ hg ci -m2
  $ echo a>a
  $ echo b>>a
  $ echo a>>a
  $ hg ci -m3
  $ hg export 2 | head -7 > ../a.patch
  $ hg export tip > out
  >>> apatch = open("../a.patch", "ab")
  >>> apatch.write("".join(open("out").readlines()[7:]))

  $ cd ..
  $ hg clone -qr0 repo3 repo3-clone
  $ cd repo3-clone
  $ hg pull -qr1 ../repo3

  $ hg import --exact ../a.patch
  applying ../a.patch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  patching file a
  Hunk #1 succeeded at 1 with fuzz 1 (offset -1 lines).
  transaction abort!
  rollback completed
  abort: patch is damaged or loses information
  [255]
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 2 changesets, 2 total revisions
