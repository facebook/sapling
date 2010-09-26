Setup

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
  [0;1mdiff -r cf9f4ba66af2 a[0m
  [0;31;1m--- a/a[0m
  [0;32;1m+++ b/a[0m
  [0;35m@@ -2,7 +2,7 @@[0m
   c
   a
   a
  [0;31m-b[0m
  [0;32m+dd[0m
   a
   a
   c

--unified=2

  $ hg diff --nodates -U 2  --color=always
  [0;1mdiff -r cf9f4ba66af2 a[0m
  [0;31;1m--- a/a[0m
  [0;32;1m+++ b/a[0m
  [0;35m@@ -3,5 +3,5 @@[0m
   a
   a
  [0;31m-b[0m
  [0;32m+dd[0m
   a
   a

diffstat

  $ hg diff --stat --color=always
   a |  2 [0;32m+[0m[0;31m-[0m
   1 files changed, 1 insertions(+), 1 deletions(-)
  $ echo "record=" >> $HGRCPATH
  $ echo "[ui]" >> $HGRCPATH
  $ echo "interactive=true" >> $HGRCPATH
  $ echo "[diff]" >> $HGRCPATH
  $ echo "git=True" >> $HGRCPATH

record

  $ chmod 0755 a
  $ hg record --color=always -m moda a <<EOF
  > y
  > y
  > EOF
  [0;1mdiff --git a/a b/a[0m
  [0;36;1mold mode 100644[0m
  [0;36;1mnew mode 100755[0m
  1 hunks, 1 lines changed
  examine changes to 'a'? [Ynsfdaq?] 
  [0;35m@@ -2,7 +2,7 @@[0m
   c
   a
   a
  [0;31m-b[0m
  [0;32m+dd[0m
   a
   a
   c
  record this change to 'a'? [Ynsfdaq?] 
  $ echo
  
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ hg rollback
  rolling back to revision 0 (undo commit)

qrecord

  $ hg qrecord --color=always -m moda patch <<EOF
  > y
  > y
  > EOF
  [0;1mdiff --git a/a b/a[0m
  [0;36;1mold mode 100644[0m
  [0;36;1mnew mode 100755[0m
  1 hunks, 1 lines changed
  examine changes to 'a'? [Ynsfdaq?] 
  [0;35m@@ -2,7 +2,7 @@[0m
   c
   a
   a
  [0;31m-b[0m
  [0;32m+dd[0m
   a
   a
   c
  record this change to 'a'? [Ynsfdaq?] 
  $ echo
  
