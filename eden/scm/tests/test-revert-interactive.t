#chg-compatible
#debugruntest-compatible

Revert interactive tests
1 add and commit file f
2 add commit file folder1/g
3 add and commit file folder2/h
4 add and commit file folder1/i
5 commit change to file f
6 commit changes to files folder1/g folder2/h
7 commit changes to files folder1/g folder2/h
8 revert interactive to commit id 2 (line 3 above), check that folder1/i is removed and
9 make workdir match 7
10 run the same test than 8 from within folder1 and check same expectations

  $ setconfig ui.interactive=true


  $ mkdir -p a/folder1 a/folder2
  $ cd a
  $ hg init
  >>> _ = open('f', 'wb').write(b"1\n2\n3\n4\n5\n")
  $ hg add f ; hg commit -m "adding f"
  $ cat f > folder1/g ; hg add folder1/g ; hg commit -m "adding folder1/g"
  $ cat f > folder2/h ; hg add folder2/h ; hg commit -m "adding folder2/h"
  $ cat f > folder1/i ; hg add folder1/i ; hg commit -m "adding folder1/i"
  >>> _ = open('f', 'wb').write(b"a\n1\n2\n3\n4\n5\nb\n")
  $ hg commit -m "modifying f"
  >>> _ = open('folder1/g', 'wb').write(b"c\n1\n2\n3\n4\n5\nd\n")
  $ hg commit -m "modifying folder1/g"
  >>> _ = open('folder2/h', 'wb').write(b"e\n1\n2\n3\n4\n5\nf\n")
  $ hg commit -m "modifying folder2/h"
  $ hg tip
  commit:      * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modifying folder2/h
  
  $ hg revert -i -r .^^^^ --all -- << EOF
  > y
  > y
  > y
  > y
  > y
  > ?
  > y
  > n
  > n
  > EOF
  reverting f
  reverting folder1/g
  removing folder1/i
  reverting folder2/h
  remove added file folder1/i (Yn)? y
  diff --git a/f b/f
  2 hunks, 2 lines changed
  examine changes to 'f'? [Ynesfdaq?] y
  
  @@ -1,6 +1,5 @@
  -a
   1
   2
   3
   4
   5
  apply change 1/6 to 'f'? [Ynesfdaq?] y
  
  @@ -2,6 +1,5 @@
   1
   2
   3
   4
   5
  -b
  apply change 2/6 to 'f'? [Ynesfdaq?] y
  
  diff --git a/folder1/g b/folder1/g
  2 hunks, 2 lines changed
  examine changes to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -1,6 +1,5 @@
  -c
   1
   2
   3
   4
   5
  apply change 3/6 to 'folder1/g'? [Ynesfdaq?] ?
  
  y - yes, apply this change
  n - no, skip this change
  e - edit this change manually
  s - skip remaining changes to this file
  f - apply remaining changes to this file
  d - done, skip remaining changes and files
  a - apply all changes to all remaining files
  q - quit, applying no changes
  ? - ? (display help)
  apply change 3/6 to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -2,6 +1,5 @@
   1
   2
   3
   4
   5
  -d
  apply change 4/6 to 'folder1/g'? [Ynesfdaq?] n
  
  diff --git a/folder2/h b/folder2/h
  2 hunks, 2 lines changed
  examine changes to 'folder2/h'? [Ynesfdaq?] n
  
  $ cat f
  1
  2
  3
  4
  5
  $ cat folder1/g
  1
  2
  3
  4
  5
  d
  $ cat folder2/h
  e
  1
  2
  3
  4
  5
  f

Test that --interactive lift the need for --all

  $ echo q | hg revert -i -r .~4
  reverting folder1/g
  reverting folder2/h
  diff --git a/folder1/g b/folder1/g
  1 hunks, 1 lines changed
  examine changes to 'folder1/g'? [Ynesfdaq?] q
  
  abort: user quit
  [255]
  $ ls folder1/
  g

Test that a noop revert doesn't do an unnecessary backup
  $ (echo y; echo n) | hg revert -i -r .~4 folder1/g
  diff --git a/folder1/g b/folder1/g
  1 hunks, 1 lines changed
  examine changes to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -3,4 +3,3 @@
   3
   4
   5
  -d
  apply this change to 'folder1/g'? [Ynesfdaq?] n
  
  $ ls folder1/
  g

Test --no-backup
  $ (echo y; echo y) | hg revert -i -C -r .~4 folder1/g
  diff --git a/folder1/g b/folder1/g
  1 hunks, 1 lines changed
  examine changes to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -3,4 +3,3 @@
   3
   4
   5
  -d
  apply this change to 'folder1/g'? [Ynesfdaq?] y
  
  $ ls folder1/
  g
  >>> _ = open('folder1/g', 'wb').write(b"1\n2\n3\n4\n5\nd\n")


  $ hg goto -C 'max(desc(modifying))'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg revert -i -r .~4 --all -- << EOF
  > n
  > y
  > y
  > y
  > y
  > y
  > n
  > n
  > EOF
  reverting f
  reverting folder1/g
  removing folder1/i
  reverting folder2/h
  remove added file folder1/i (Yn)? n
  diff --git a/f b/f
  2 hunks, 2 lines changed
  examine changes to 'f'? [Ynesfdaq?] y
  
  @@ -1,6 +1,5 @@
  -a
   1
   2
   3
   4
   5
  apply change 1/6 to 'f'? [Ynesfdaq?] y
  
  @@ -2,6 +1,5 @@
   1
   2
   3
   4
   5
  -b
  apply change 2/6 to 'f'? [Ynesfdaq?] y
  
  diff --git a/folder1/g b/folder1/g
  2 hunks, 2 lines changed
  examine changes to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -1,6 +1,5 @@
  -c
   1
   2
   3
   4
   5
  apply change 3/6 to 'folder1/g'? [Ynesfdaq?] y
  
  @@ -2,6 +1,5 @@
   1
   2
   3
   4
   5
  -d
  apply change 4/6 to 'folder1/g'? [Ynesfdaq?] n
  
  diff --git a/folder2/h b/folder2/h
  2 hunks, 2 lines changed
  examine changes to 'folder2/h'? [Ynesfdaq?] n
  
  $ cat f
  1
  2
  3
  4
  5
  $ cat folder1/g
  1
  2
  3
  4
  5
  d
  $ cat folder2/h
  e
  1
  2
  3
  4
  5
  f
  $ hg st
  M f
  M folder1/g
  $ hg revert --interactive f << EOF
  > y
  > ?
  > y
  > n
  > n
  > EOF
  diff --git a/f b/f
  2 hunks, 2 lines changed
  examine changes to 'f'? [Ynesfdaq?] y
  
  @@ -1,6 +1,5 @@
  -a
   1
   2
   3
   4
   5
  discard change 1/2 to 'f'? [Ynesfdaq?] ?
  
  y - yes, discard this change
  n - no, skip this change
  e - edit this change manually
  s - skip remaining changes to this file
  f - discard remaining changes to this file
  d - done, skip remaining changes and files
  a - discard all changes to all remaining files
  q - quit, discarding no changes
  ? - ? (display help)
  discard change 1/2 to 'f'? [Ynesfdaq?] y
  
  @@ -2,6 +1,5 @@
   1
   2
   3
   4
   5
  -b
  discard change 2/2 to 'f'? [Ynesfdaq?] n
  
  $ hg st
  M f
  M folder1/g
  ? f.orig
  $ cat f
  a
  1
  2
  3
  4
  5
  $ cat f.orig
  1
  2
  3
  4
  5
  $ rm f.orig
  $ hg goto -C .
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Check editing files newly added by a revert

1) Create a dummy editor changing 1 to 42
  $ cat > $TESTTMP/editor.sh << '__EOF__'
  > cat "$1"  | sed "s/1/42/g"  > tt
  > mv tt  "$1"
  > __EOF__

2) Add k
  $ printf "1\n" > k
  $ hg add k
  $ hg commit -m "add k"

3) Use interactive revert with editing (replacing +1 with +42):
  $ printf "0\n2\n" > k
  $ HGEDITOR="\"sh\" \"${TESTTMP}/editor.sh\"" hg revert -i  <<EOF
  > y
  > e
  > EOF
  reverting k
  diff --git a/k b/k
  1 hunks, 2 lines changed
  examine changes to 'k'? [Ynesfdaq?] y
  
  @@ -1,1 +1,2 @@
  -1
  +0
  +2
  discard this change to 'k'? [Ynesfdaq?] e
  
  $ cat k
  42

  $ hg goto -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg purge
  $ touch newfile
  $ hg add newfile
  $ hg status
  A newfile
  $ hg revert -i <<EOF
  > n
  > EOF
  forgetting newfile
  forget added file newfile (Yn)? n
  $ hg status
  A newfile
  $ hg revert -i <<EOF
  > y
  > EOF
  forgetting newfile
  forget added file newfile (Yn)? y
  $ hg status
  ? newfile

When a line without EOL is selected during "revert -i" (issue5651)

  $ hg init $TESTTMP/revert-i-eol
  $ cd $TESTTMP/revert-i-eol
  $ echo 0 > a
  $ hg ci -qAm 0
  $ printf 1 >> a
  $ hg ci -qAm 1
  $ cat a
  0
  1 (no-eol)

  $ hg revert -ir'.^' <<EOF
  > y
  > y
  > EOF
  reverting a
  diff --git a/a b/a
  1 hunks, 1 lines changed
  examine changes to 'a'? [Ynesfdaq?] y
  
  @@ -1,2 +1,1 @@
   0
  -1
  \ No newline at end of file
  apply this change to 'a'? [Ynesfdaq?] y
  
  $ cat a
  0

  $ cd ..
