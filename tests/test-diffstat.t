  $ hg init repo
  $ cd repo
  $ i=0; while [ "$i" -lt 213 ]; do echo a >> a; i=`expr $i + 1`; done
  $ hg add a
  $ cp a b
  $ hg add b

Wide diffstat:

  $ hg diff --stat
   a |  213 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
   b |  213 ++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++++
   2 files changed, 426 insertions(+), 0 deletions(-)

diffstat width:

  $ COLUMNS=24 hg diff --config ui.interactive=true --stat
   a |  213 ++++++++++++++
   b |  213 ++++++++++++++
   2 files changed, 426 insertions(+), 0 deletions(-)

  $ hg ci -m adda

  $ cat >> a <<EOF
  > a
  > a
  > a
  > EOF

Narrow diffstat:

  $ hg diff --stat
   a |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)

  $ hg ci -m appenda

  >>> open("c", "wb").write("\0")
  $ touch d
  $ hg add c d

Binary diffstat:

  $ hg diff --stat
   c |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

Binary git diffstat:

  $ hg diff --stat --git
   c |  Bin 
   d |    0 
   2 files changed, 0 insertions(+), 0 deletions(-)

  $ hg ci -m createb

  >>> open("file with spaces", "wb").write("\0")
  $ hg add "file with spaces"

Filename with spaces diffstat:

  $ hg diff --stat
   file with spaces |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

Filename with spaces git diffstat:

  $ hg diff --stat --git
   file with spaces |  Bin 
   1 files changed, 0 insertions(+), 0 deletions(-)

Filename without "a/" or "b/" (issue5759):

  $ hg diff --config 'diff.noprefix=1' -c1 --stat --git
   a |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)
  $ hg diff --config 'diff.noprefix=1' -c2 --stat --git
   c |  Bin 
   d |    0 
   2 files changed, 0 insertions(+), 0 deletions(-)

  $ hg log --config 'diff.noprefix=1' -r '1:' -p --stat --git
  changeset:   1:3a95b07bb77f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     appenda
  
   a |  3 +++
   1 files changed, 3 insertions(+), 0 deletions(-)
  
  diff --git a a
  --- a
  +++ a
  @@ -211,3 +211,6 @@
   a
   a
   a
  +a
  +a
  +a
  
  changeset:   2:c60a6c753773
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     createb
  
   c |  Bin 
   d |    0 
   2 files changed, 0 insertions(+), 0 deletions(-)
  
  diff --git c c
  new file mode 100644
  index e69de29bb2d1d6434b8b29ae775ad8c2e48c5391..f76dd238ade08917e6712764a16a22005a50573d
  GIT binary patch
  literal 1
  Ic${MZ000310RR91
  
  diff --git d d
  new file mode 100644
  

diffstat within directories:

  $ hg rm -f 'file with spaces'

  $ mkdir dir1 dir2
  $ echo new1 > dir1/new
  $ echo new2 > dir2/new
  $ hg add dir1/new dir2/new
  $ hg diff --stat
   dir1/new |  1 +
   dir2/new |  1 +
   2 files changed, 2 insertions(+), 0 deletions(-)

  $ hg diff --stat --root dir1
   new |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ hg diff --stat --root dir1 dir2
  warning: dir2 not inside relative root dir1

  $ hg diff --stat --root dir1 -I dir1/old

  $ cd dir1
  $ hg diff --stat .
   dir1/new |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  $ hg diff --stat --root .
   new |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ hg diff --stat --root ../dir1 ../dir2
  warning: ../dir2 not inside relative root . (glob)

  $ hg diff --stat --root . -I old

  $ cd ..

Files with lines beginning with '--' or '++' should be properly counted in diffstat

  $ hg up -Cr tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm dir1/new
  $ rm dir2/new
  $ rm "file with spaces"
  $ cat > file << EOF
  > line 1
  > line 2
  > line 3
  > EOF
  $ hg commit -Am file
  adding file

Lines added starting with '--' should count as additions
  $ cat > file << EOF
  > line 1
  > -- line 2, with dashes
  > line 3
  > EOF

  $ hg diff --root .
  diff -r be1569354b24 file
  --- a/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file	* (glob)
  @@ -1,3 +1,3 @@
   line 1
  -line 2
  +-- line 2, with dashes
   line 3

  $ hg diff --root . --stat
   file |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)

Lines changed starting with '--' should count as deletions
  $ hg commit -m filev2
  $ cat > file << EOF
  > line 1
  > -- line 2, with dashes, changed again
  > line 3
  > EOF

  $ hg diff --root .
  diff -r 160f7c034df6 file
  --- a/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file	* (glob)
  @@ -1,3 +1,3 @@
   line 1
  --- line 2, with dashes
  +-- line 2, with dashes, changed again
   line 3

  $ hg diff --root . --stat
   file |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)

Lines changed starting with '--' should count as deletions
and starting with '++' should count as additions
  $ cat > file << EOF
  > line 1
  > ++ line 2, switched dashes to plusses
  > line 3
  > EOF

  $ hg diff --root .
  diff -r 160f7c034df6 file
  --- a/file	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file	* (glob)
  @@ -1,3 +1,3 @@
   line 1
  --- line 2, with dashes
  +++ line 2, switched dashes to plusses
   line 3

  $ hg diff --root . --stat
   file |  2 +-
   1 files changed, 1 insertions(+), 1 deletions(-)
