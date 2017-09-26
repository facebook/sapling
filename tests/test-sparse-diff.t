  $ hg init repo
  $ cd repo
  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > sparse=$TESTDIR/../hgext3rd/sparse.py
  > EOF
  $ mkdir a b
  $ echo a1 > a/a
  $ echo b1 > b/b
  $ hg add a/a b/b
  $ hg commit -m "add files"
  $ echo a2 > a/a
  $ echo b2 > b/b
  $ hg commit -m "modify files"
  $ hg sparse --exclude b

Run diff.  This should still show the file contents of excluded files (and should not crash).

  $ hg diff -r ".^"
  diff -r 45479a47b024 a/a
  --- a/a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a1
  +a2
  diff -r 45479a47b024 b/b
  --- a/b/b	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b/b	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -b1
  +b2
