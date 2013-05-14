Setup

  $ echo "[color]" >> $HGRCPATH
  $ echo "mode = ansi" >> $HGRCPATH
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ hg init repo
  $ cd repo
  $ cat > a <<EOF
  > c
  > c
  > a
  > a
  > b
  > a
  > a
  > c
  > c
  > EOF
  $ hg ci -Am adda
  adding a
  $ cat > a <<EOF
  > c
  > c
  > a
  > a
  > dd
  > a
  > a
  > c
  > c
  > EOF

default context

  $ hg diff --nodates --color=always
  \x1b[0;1mdiff -r cf9f4ba66af2 a\x1b[0m (esc)
  \x1b[0;31;1m--- a/a\x1b[0m (esc)
  \x1b[0;32;1m+++ b/a\x1b[0m (esc)
  \x1b[0;35m@@ -2,7 +2,7 @@\x1b[0m (esc)
   c
   a
   a
  \x1b[0;31m-b\x1b[0m (esc)
  \x1b[0;32m+dd\x1b[0m (esc)
   a
   a
   c

--unified=2

  $ hg diff --nodates -U 2  --color=always
  \x1b[0;1mdiff -r cf9f4ba66af2 a\x1b[0m (esc)
  \x1b[0;31;1m--- a/a\x1b[0m (esc)
  \x1b[0;32;1m+++ b/a\x1b[0m (esc)
  \x1b[0;35m@@ -3,5 +3,5 @@\x1b[0m (esc)
   a
   a
  \x1b[0;31m-b\x1b[0m (esc)
  \x1b[0;32m+dd\x1b[0m (esc)
   a
   a

diffstat

  $ hg diff --stat --color=always
   a |  2 \x1b[0;32m+\x1b[0m\x1b[0;31m-\x1b[0m (esc)
   1 files changed, 1 insertions(+), 1 deletions(-)
  $ echo "record=" >> $HGRCPATH
  $ echo "[ui]" >> $HGRCPATH
  $ echo "interactive=true" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "git=True" >> $HGRCPATH

#if execbit

record

  $ chmod +x a
  $ hg record --color=always -m moda a <<EOF
  > y
  > y
  > EOF
  \x1b[0;1mdiff --git a/a b/a\x1b[0m (esc)
  \x1b[0;36;1mold mode 100644\x1b[0m (esc)
  \x1b[0;36;1mnew mode 100755\x1b[0m (esc)
  1 hunks, 1 lines changed
  \x1b[0;33mexamine changes to 'a'? [Ynesfdaq?]\x1b[0m  (esc)
  \x1b[0;35m@@ -2,7 +2,7 @@\x1b[0m (esc)
   c
   a
   a
  \x1b[0;31m-b\x1b[0m (esc)
  \x1b[0;32m+dd\x1b[0m (esc)
   a
   a
   c
  \x1b[0;33mrecord this change to 'a'? [Ynesfdaq?]\x1b[0m  (esc)

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg rollback
  repository tip rolled back to revision 0 (undo commit)
  working directory now based on revision 0

qrecord

  $ hg qrecord --color=always -m moda patch <<EOF
  > y
  > y
  > EOF
  \x1b[0;1mdiff --git a/a b/a\x1b[0m (esc)
  \x1b[0;36;1mold mode 100644\x1b[0m (esc)
  \x1b[0;36;1mnew mode 100755\x1b[0m (esc)
  1 hunks, 1 lines changed
  \x1b[0;33mexamine changes to 'a'? [Ynesfdaq?]\x1b[0m  (esc)
  \x1b[0;35m@@ -2,7 +2,7 @@\x1b[0m (esc)
   c
   a
   a
  \x1b[0;31m-b\x1b[0m (esc)
  \x1b[0;32m+dd\x1b[0m (esc)
   a
   a
   c
  \x1b[0;33mrecord this change to 'a'? [Ynesfdaq?]\x1b[0m  (esc)

  $ hg qpop -a
  popping patch
  patch queue now empty

#endif

issue3712: test colorization of subrepo diff

  $ hg init sub
  $ echo b > sub/b
  $ hg -R sub commit -Am 'create sub'
  adding b
  $ echo 'sub = sub' > .hgsub
  $ hg add .hgsub
  $ hg commit -m 'add subrepo sub'
  $ echo aa >> a
  $ echo bb >> sub/b

  $ hg diff --color=always -S
  \x1b[0;1mdiff --git a/a b/a\x1b[0m (esc)
  \x1b[0;31;1m--- a/a\x1b[0m (esc)
  \x1b[0;32;1m+++ b/a\x1b[0m (esc)
  \x1b[0;35m@@ -7,3 +7,4 @@\x1b[0m (esc)
   a
   c
   c
  \x1b[0;32m+aa\x1b[0m (esc)
  \x1b[0;1mdiff --git a/sub/b b/sub/b\x1b[0m (esc)
  \x1b[0;31;1m--- a/sub/b\x1b[0m (esc)
  \x1b[0;32;1m+++ b/sub/b\x1b[0m (esc)
  \x1b[0;35m@@ -1,1 +1,2 @@\x1b[0m (esc)
   b
  \x1b[0;32m+bb\x1b[0m (esc)

  $ cd ..
