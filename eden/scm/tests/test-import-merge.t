#chg-compatible

  $ disable treemanifest
  $ tipparents() {
  > hg parents --template "{node|short} {desc|firstline}\n" -r tip
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
  $ hg up 'desc(adda)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ hg up 'desc(changea)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 'desc(addb)'
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
  added 1 changesets with 1 changes to 1 files

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
  $ hg debugstrip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test without --exact and diff.p1 != workingdir.p1

  $ hg up 'desc(addc)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import ../merge.diff
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ tipparents
  890ecaa90481 addc
  $ hg debugstrip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test with --exact

  $ hg import --exact ../merge.diff
  applying ../merge.diff
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg debugstrip --no-backup tip
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test with --bypass and diff.p1 == workingdir.p1

  $ hg up 'desc(changea)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff
  applying ../merge.diff
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg debugstrip --no-backup tip

Test with --bypass and diff.p1 != workingdir.p1

  $ hg up 'desc(addc)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg import --bypass ../merge.diff
  applying ../merge.diff
  warning: import the patch as a normal revision
  (use --exact to import the patch as a merge)
  $ tipparents
  890ecaa90481 addc
  $ hg debugstrip --no-backup tip

Test with --bypass and --exact

  $ hg import --bypass --exact ../merge.diff
  applying ../merge.diff
  $ tipparents
  540395c44225 changea
  102a90ea7b4a addb
  $ hg debugstrip --no-backup tip

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
  $ hg export 'desc(2)' | head -7 > ../a.patch
  $ hg export tip > out
  >>> apatch = open("../a.patch", "ab")
  >>> _ = apatch.write(b"".join(open("out", "rb").readlines()[7:]))

  $ cd ..
  $ hg clone -qr0 repo3 repo3-clone
  $ cd repo3-clone
  $ hg pull -qr1 ../repo3

  $ hg import --exact ../a.patch
  applying ../a.patch
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  patching file a
  Hunk #1 succeeded at 1 with fuzz 1 (offset -1 lines).
  abort: patch is damaged or loses information
  [255]
  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  1 files, 3 changesets, 3 total revisions
