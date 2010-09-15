
# some implementations of cp can't create hardlinks
  $ cat > cp.py <<EOF
  > from mercurial import util
  > import sys
  > util.copyfiles(sys.argv[1], sys.argv[2], hardlink=True)
  > EOF

Test hardlinking outside hg:

  $ mkdir x
  $ echo foo > x/a

  $ python cp.py x y
  $ echo bar >> y/a

No diff if hardlink:

  $ diff x/a y/a

Test mq hardlinking:

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH

  $ hg init a
  $ cd a

  $ hg qimport -n foo - << EOF
  > # HG changeset patch
  > # Date 1 0
  > diff -r 2588a8b53d66 a
  > --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/a	Wed Jul 23 15:54:29 2008 +0200
  > @@ -0,0 +1,1 @@
  > +a
  > EOF
  adding foo to series file

  $ hg qpush
  applying foo
  now at: foo

  $ cd ..
  $ python cp.py a b
  $ cd b

  $ hg qimport -n bar - << EOF
  > # HG changeset patch
  > # Date 2 0
  > diff -r 2588a8b53d66 a
  > --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  > +++ b/b	Wed Jul 23 15:54:29 2008 +0200
  > @@ -0,0 +1,1 @@
  > +b
  > EOF
  adding bar to series file

  $ hg qpush
  applying bar
  now at: bar

  $ cat .hg/patches/status
  430ed4828a74fa4047bc816a25500f7472ab4bfe:foo
  4e7abb4840c46a910f6d7b4d3c3fc7e5209e684c:bar

  $ cat .hg/patches/series
  foo
  bar

  $ cat ../a/.hg/patches/status
  430ed4828a74fa4047bc816a25500f7472ab4bfe:foo

  $ cat ../a/.hg/patches/series
  foo

Test tags hardlinking:

  $ hg qdel -r qbase:qtip
  patch foo finalized without changeset message
  patch bar finalized without changeset message

  $ hg tag -l lfoo
  $ hg tag foo

  $ cd ..
  $ python cp.py b c
  $ cd c

  $ hg tag -l -r 0 lbar
  $ hg tag -r 0 bar

  $ cat .hgtags
  4e7abb4840c46a910f6d7b4d3c3fc7e5209e684c foo
  430ed4828a74fa4047bc816a25500f7472ab4bfe bar

  $ cat .hg/localtags
  4e7abb4840c46a910f6d7b4d3c3fc7e5209e684c lfoo
  430ed4828a74fa4047bc816a25500f7472ab4bfe lbar

  $ cat ../b/.hgtags
  4e7abb4840c46a910f6d7b4d3c3fc7e5209e684c foo

  $ cat ../b/.hg/localtags
  4e7abb4840c46a910f6d7b4d3c3fc7e5209e684c lfoo

