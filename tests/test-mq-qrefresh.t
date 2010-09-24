  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "nodates=1" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ mkdir 1 2
  $ echo 'base' > 1/base
  $ echo 'base' > 2/base
  $ hg ci -Ambase
  adding 1/base
  adding 2/base

  $ hg qnew -mmqbase mqbase

  $ echo 'patched' > 1/base
  $ echo 'patched' > 2/base
  $ hg qrefresh

  $ hg qdiff
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg qdiff .
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ echo 'patched again' > base
  $ hg qrefresh 1

  $ hg qdiff
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg qdiff .
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched

qrefresh . in subdir:

  $ ( cd 1 ; hg qrefresh . )

  $ hg qdiff
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg qdiff .
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched

qrefresh in hg-root again:

  $ hg qrefresh

  $ hg qdiff
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg qdiff .
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched


qrefresh --short tests:

  $ echo 'orphan' > orphanchild
  $ hg add orphanchild
  $ hg qrefresh nonexistingfilename # clear patch
  $ hg qrefresh --short 1/base
  $ hg qrefresh --short 2/base

  $ hg qdiff
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 orphanchild
  --- /dev/null
  +++ b/orphanchild
  @@ -0,0 +1,1 @@
  +orphan

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ hg st
  A orphanchild
  ? base

diff shows what is not in patch:

  $ hg diff
  diff -r ???????????? orphanchild (glob)
  --- /dev/null
  +++ b/orphanchild
  @@ -0,0 +1,1 @@
  +orphan

Before starting exclusive tests:

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

Exclude 2/base:

  $ hg qref -s -X 2/base

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched

status shows 2/base as dirty:

  $ hg status
  M 2/base
  A orphanchild
  ? base

Remove 1/base and add 2/base again but not orphanchild:

  $ hg qref -s -X orphanchild -X 1/base 2/base orphanchild

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 2/base
  --- a/2/base
  +++ b/2/base
  @@ -1,1 +1,1 @@
  -base
  +patched

Add 1/base with include filter - and thus remove 2/base from patch:

  $ hg qref -s -I 1/ o* */*

  $ cat .hg/patches/mqbase
  # HG changeset patch
  # Parent e7af5904b465cd1f4f3cf6b26fe14e8db6f63eaa
  mqbase
  
  diff -r e7af5904b465 1/base
  --- a/1/base
  +++ b/1/base
  @@ -1,1 +1,1 @@
  -base
  +patched

  $ cd ..


Test qrefresh --git losing copy metadata:

  $ hg init repo
  $ cd repo

  $ echo "[diff]" >> .hg/hgrc
  $ echo "git=True" >> .hg/hgrc
  $ echo a > a

  $ hg ci -Am adda
  adding a
  $ hg copy a ab
  $ echo b >> ab
  $ hg copy a ac
  $ echo c >> ac

Capture changes:

  $ hg qnew -f p1

  $ hg qdiff
  diff --git a/a b/ab
  copy from a
  copy to ab
  --- a/a
  +++ b/ab
  @@ -1,1 +1,2 @@
   a
  +b
  diff --git a/a b/ac
  copy from a
  copy to ac
  --- a/a
  +++ b/ac
  @@ -1,1 +1,2 @@
   a
  +c

Refresh and check changes again:

  $ hg qrefresh

  $ hg qdiff
  diff --git a/a b/ab
  copy from a
  copy to ab
  --- a/a
  +++ b/ab
  @@ -1,1 +1,2 @@
   a
  +b
  diff --git a/a b/ac
  copy from a
  copy to ac
  --- a/a
  +++ b/ac
  @@ -1,1 +1,2 @@
   a
  +c

  $ cd ..


Issue1441: qrefresh confused after hg rename:

  $ hg init repo-1441
  $ cd repo-1441
  $ echo a > a
  $ hg add a
  $ hg qnew -f p
  $ hg mv a b
  $ hg qrefresh

  $ hg qdiff
  diff -r 000000000000 b
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +a

  $ cd ..


Issue2025: qrefresh does not honor filtering options when tip !=
qtip:

  $ hg init repo-2025
  $ cd repo-2025
  $ echo a > a
  $ echo b > b
  $ hg ci -qAm addab
  $ echo a >> a
  $ echo b >> b
  $ hg qnew -f patch
  $ hg up -qC 0
  $ echo c > c
  $ hg ci -qAm addc
  $ hg up -qC 1

refresh with tip != qtip:

  $ hg --config diff.nodates=1 qrefresh -I b

  $ hg st
  M a

  $ cat b
  b
  b

  $ cat .hg/patches/patch
  # HG changeset patch
  # Parent 1a60229be7ac3e4a7f647508e99b87bef1f03593
  
  diff -r 1a60229be7ac b
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
   b
  +b

  $ cd ..


Issue1441 with git patches:

  $ hg init repo-1441-git
  $ cd repo-1441-git

  $ echo "[diff]" >> .hg/hgrc
  $ echo "git=True" >> .hg/hgrc

  $ echo a > a
  $ hg add a
  $ hg qnew -f p
  $ hg mv a b
  $ hg qrefresh

  $ hg qdiff --nodates
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +a

  $ cd ..

