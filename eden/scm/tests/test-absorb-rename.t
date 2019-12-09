#chg-compatible

TODO: enable obsstore
  $ cat >> $HGRCPATH << EOF
  > [diff]
  > git=1
  > [extensions]
  > absorb=
  > [experimental]
  > evolution=
  > EOF

  $ sedi() { # workaround check-code
  > pattern="$1"
  > shift
  > for i in "$@"; do
  >     sed "$pattern" "$i" > "$i".tmp
  >     mv "$i".tmp "$i"
  > done
  > }

rename a to b, then b to a

  $ hg init repo1
  $ cd repo1

  $ echo 1 > a
  $ hg ci -A a -m 1
  $ hg mv a b
  $ echo 2 >> b
  $ hg ci -m 2
  $ hg mv b a
  $ echo 3 >> a
  $ hg ci -m 3

  $ hg annotate -ncf a
  0 eff892de26ec a: 1
  1 bf56e1f4f857 b: 2
  2 0b888b00216c a: 3

  $ sedi 's/$/a/' a
  $ hg absorb -aq

  $ hg status

  $ hg annotate -ncf a
  0 5d1c5620e6f2 a: 1a
  1 9a14ffe67ae9 b: 2a
  2 9191d121a268 a: 3a

when the first changeset is public

  $ hg phase --public -r 0

  $ sedi 's/a/A/' a

  $ hg absorb -aq

  $ hg diff
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,3 +1,3 @@
  -1a
  +1A
   2A
   3A

copy a to b

  $ cd ..
  $ hg init repo2
  $ cd repo2

  $ echo 1 > a
  $ hg ci -A a -m 1
  $ hg cp a b
  $ echo 2 >> b
  $ hg ci -m 2

  $ hg log -T '{rev}:{node|short} {desc}\n'
  1:17b72129ab68 2
  0:eff892de26ec 1

  $ sedi 's/$/a/' a
  $ sedi 's/$/b/' b

  $ hg absorb -aq

  $ hg diff
  diff --git a/b b/b
  --- a/b
  +++ b/b
  @@ -1,2 +1,2 @@
  -1
  +1b
   2b

copy b to a

  $ cd ..
  $ hg init repo3
  $ cd repo3

  $ echo 1 > b
  $ hg ci -A b -m 1
  $ hg cp b a
  $ echo 2 >> a
  $ hg ci -m 2

  $ hg log -T '{rev}:{node|short} {desc}\n'
  1:e62c256d8b24 2
  0:55105f940d5c 1

  $ sedi 's/$/a/' a
  $ sedi 's/$/a/' b

  $ hg absorb -aq

  $ hg diff
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,2 +1,2 @@
  -1
  +1a
   2a

"move" b to both a and c, follow a - sorted alphabetically

  $ cd ..
  $ hg init repo4
  $ cd repo4

  $ echo 1 > b
  $ hg ci -A b -m 1
  $ hg cp b a
  $ hg cp b c
  $ hg rm b
  $ echo 2 >> a
  $ echo 3 >> c
  $ hg commit -m cp

  $ hg log -T '{rev}:{node|short} {desc}\n'
  1:366daad8e679 cp
  0:55105f940d5c 1

  $ sedi 's/$/a/' a
  $ sedi 's/$/c/' c

  $ hg absorb -aq

  $ hg log -G -p -T '{rev}:{node|short} {desc}\n'
  @  1:70606019f91b cp
  |  diff --git a/b b/a
  |  rename from b
  |  rename to a
  |  --- a/b
  |  +++ b/a
  |  @@ -1,1 +1,2 @@
  |   1a
  |  +2a
  |  diff --git a/b b/c
  |  copy from b
  |  copy to c
  |  --- a/b
  |  +++ b/c
  |  @@ -1,1 +1,2 @@
  |  -1a
  |  +1
  |  +3c
  |
  o  0:bfb67c3539c1 1
     diff --git a/b b/b
     new file mode 100644
     --- /dev/null
     +++ b/b
     @@ -0,0 +1,1 @@
     +1a
  
run absorb again would apply the change to c

  $ hg absorb -aq

  $ hg log -G -p -T '{rev}:{node|short} {desc}\n'
  @  1:8bd536cce368 cp
  |  diff --git a/b b/a
  |  rename from b
  |  rename to a
  |  --- a/b
  |  +++ b/a
  |  @@ -1,1 +1,2 @@
  |   1a
  |  +2a
  |  diff --git a/b b/c
  |  copy from b
  |  copy to c
  |  --- a/b
  |  +++ b/c
  |  @@ -1,1 +1,2 @@
  |  -1a
  |  +1c
  |  +3c
  |
  o  0:bfb67c3539c1 1
     diff --git a/b b/b
     new file mode 100644
     --- /dev/null
     +++ b/b
     @@ -0,0 +1,1 @@
     +1a
  
"move" b to a, c and d, follow d if a gets renamed to e, and c is deleted

  $ cd ..
  $ hg init repo5
  $ cd repo5

  $ echo 1 > b
  $ hg ci -A b -m 1
  $ hg cp b a
  $ hg cp b c
  $ hg cp b d
  $ hg rm b
  $ echo 2 >> a
  $ echo 3 >> c
  $ echo 4 >> d
  $ hg commit -m cp
  $ hg mv a e
  $ hg rm c
  $ hg commit -m mv

  $ hg log -T '{rev}:{node|short} {desc}\n'
  2:49911557c471 mv
  1:7bc3d43ede83 cp
  0:55105f940d5c 1

  $ sedi 's/$/e/' e
  $ sedi 's/$/d/' d

  $ hg absorb -aq

  $ hg diff
  diff --git a/e b/e
  --- a/e
  +++ b/e
  @@ -1,2 +1,2 @@
  -1
  +1e
   2e

  $ hg log -G -p -T '{rev}:{node|short} {desc}\n'
  @  2:34be9b0c786e mv
  |  diff --git a/c b/c
  |  deleted file mode 100644
  |  --- a/c
  |  +++ /dev/null
  |  @@ -1,2 +0,0 @@
  |  -1
  |  -3
  |  diff --git a/a b/e
  |  rename from a
  |  rename to e
  |
  o  1:13e56db5948d cp
  |  diff --git a/b b/a
  |  rename from b
  |  rename to a
  |  --- a/b
  |  +++ b/a
  |  @@ -1,1 +1,2 @@
  |  -1d
  |  +1
  |  +2e
  |  diff --git a/b b/c
  |  copy from b
  |  copy to c
  |  --- a/b
  |  +++ b/c
  |  @@ -1,1 +1,2 @@
  |  -1d
  |  +1
  |  +3
  |  diff --git a/b b/d
  |  copy from b
  |  copy to d
  |  --- a/b
  |  +++ b/d
  |  @@ -1,1 +1,2 @@
  |   1d
  |  +4d
  |
  o  0:0037613a5dc6 1
     diff --git a/b b/b
     new file mode 100644
     --- /dev/null
     +++ b/b
     @@ -0,0 +1,1 @@
     +1d
  
