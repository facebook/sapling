  $ cat >> $HGRCPATH << EOF
  > [diff]
  > git = true
  > EOF

  $ hg init
  $ cat > foo << EOF
  > 0
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -Am init
  adding foo
  $ cat > foo << EOF
  > 0
  > 0
  > 0
  > 0
  > 1
  > 2
  > 3
  > 4
  > EOF
  $ hg ci -m 'more 0'
  $ sed 's/2/2+/' foo > foo.new
  $ mv foo.new foo
  $ cat > bar << EOF
  > a
  > b
  > c
  > d
  > e
  > EOF
  $ hg add bar
  $ hg ci -Am "2 -> 2+; added bar"
  $ cat >> foo << EOF
  > 5
  > 6
  > 7
  > 8
  > 9
  > 10
  > 11
  > EOF
  $ hg ci -m "to 11"

Add some changes with two diff hunks

  $ sed 's/^1$/ 1/' foo > foo.new
  $ mv foo.new foo
  $ sed 's/^11$/11+/' foo > foo.new
  $ mv foo.new foo
  $ hg ci -m '11 -> 11+; leading space before "1"'
(make sure there are two hunks in "foo")
  $ hg diff -c .
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  @@ -12,4 +12,4 @@
   8
   9
   10
  -11
  +11+
  $ sed 's/3/3+/' foo > foo.new
  $ mv foo.new foo
  $ sed 's/^11+$/11-/' foo > foo.new
  $ mv foo.new foo
  $ sed 's/a/a+/' bar > bar.new
  $ mv bar.new bar
  $ hg ci -m 'foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+'
(make sure there are two hunks in "foo")
  $ hg diff -c . foo
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  @@ -12,4 +12,4 @@
   8
   9
   10
  -11+
  +11-

  $ hg log -f -L foo,5:7 -p
  changeset:   5:cfdf972b3971
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  
  changeset:   4:eaec41c1a0c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     11 -> 11+; leading space before "1"
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

With --template.

  $ hg log -f -L foo,5:7 -T '{rev}:{node|short} {desc|firstline}\n'
  5:cfdf972b3971 foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  4:eaec41c1a0c9 11 -> 11+; leading space before "1"
  2:63a884426fd0 2 -> 2+; added bar
  0:5ae1f82b9a00 init
  $ hg log -f -L foo,5:7 -T json
  [
   {
    "rev": 5,
    "node": "cfdf972b3971a2a59638bf9583c0debbffee5404",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+",
    "bookmarks": [],
    "tags": ["tip"],
    "parents": ["eaec41c1a0c9ad0a5e999611d0149d171beffb8c"]
   },
   {
    "rev": 4,
    "node": "eaec41c1a0c9ad0a5e999611d0149d171beffb8c",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "11 -> 11+; leading space before \"1\"",
    "bookmarks": [],
    "tags": [],
    "parents": ["730a61fbaecf426c17c2c66bc42d195b5d5b0ba8"]
   },
   {
    "rev": 2,
    "node": "63a884426fd0b277fcd55895bbb2f230434576eb",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "2 -> 2+; added bar",
    "bookmarks": [],
    "tags": [],
    "parents": ["29a1e7c6b80024f63f310a2d71de979e9d2996d7"]
   },
   {
    "rev": 0,
    "node": "5ae1f82b9a000ff1e0967d0dac1c58b9d796e1b4",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "init",
    "bookmarks": [],
    "tags": [],
    "parents": ["0000000000000000000000000000000000000000"]
   }
  ]

With some white-space diff option, respective revisions are skipped.

  $ hg log -f -L foo,5:7 -p --config diff.ignorews=true
  changeset:   5:cfdf972b3971
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

Regular file patterns are not allowed.

  $ hg log -f -L foo,5:7 -p bar
  abort: FILE arguments are not compatible with --line-range option
  [255]

Option --rev acts as a restriction.

  $ hg log -f -L foo,5:7 -p -r 'desc(2)'
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

With several -L patterns, changes touching any files in their respective line
range are show.

  $ hg log -f -L foo,5:7 -L bar,1:2 -p
  changeset:   5:cfdf972b3971
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/bar b/bar
  --- a/bar
  +++ b/bar
  @@ -1,4 +1,4 @@
  -a
  +a+
   b
   c
   d
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  
  changeset:   4:eaec41c1a0c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     11 -> 11+; leading space before "1"
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/bar b/bar
  new file mode 100644
  --- /dev/null
  +++ b/bar
  @@ -0,0 +1,5 @@
  +a
  +b
  +c
  +d
  +e
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

Multiple -L options with the same file yields changes touching any of
specified line ranges.

  $ hg log -f -L foo,5:7 -L foo,14:15 -p
  changeset:   5:cfdf972b3971
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  @@ -12,4 +12,4 @@
   8
   9
   10
  -11+
  +11-
  
  changeset:   4:eaec41c1a0c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     11 -> 11+; leading space before "1"
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  @@ -12,4 +12,4 @@
   8
   9
   10
  -11
  +11+
  
  changeset:   3:730a61fbaecf
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     to 11
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -6,3 +6,10 @@
   2+
   3
   4
  +5
  +6
  +7
  +8
  +9
  +10
  +11
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

A file with a comma in its name.

  $ cat > ba,z << EOF
  > q
  > w
  > e
  > r
  > t
  > y
  > EOF
  $ hg ci -Am 'querty'
  adding ba,z
  $ cat >> ba,z << EOF
  > u
  > i
  > o
  > p
  > EOF
  $ hg ci -m 'more keys'
  $ cat > ba,z << EOF
  > a
  > z
  > e
  > r
  > t
  > y
  > u
  > i
  > o
  > p
  > EOF
  $ hg ci -m 'azerty'
  $ hg log -f -L ba,z,1:2 -p
  changeset:   8:52373265138b
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     azerty
  
  diff --git a/ba,z b/ba,z
  --- a/ba,z
  +++ b/ba,z
  @@ -1,5 +1,5 @@
  -q
  -w
  +a
  +z
   e
   r
   t
  
  changeset:   6:96ba8850f316
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     querty
  
  diff --git a/ba,z b/ba,z
  new file mode 100644
  --- /dev/null
  +++ b/ba,z
  @@ -0,0 +1,6 @@
  +q
  +w
  +e
  +r
  +t
  +y
  

Exact prefix kinds work in -L options.

  $ mkdir dir
  $ cd dir
  $ hg log -f -L path:foo,5:7 -p
  changeset:   5:cfdf972b3971
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  
  changeset:   4:eaec41c1a0c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     11 -> 11+; leading space before "1"
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

Renames are followed.

  $ hg mv ../foo baz
  $ sed 's/1/1+/' baz > baz.new
  $ mv baz.new baz
  $ hg ci -m 'foo -> dir/baz; 1-1+'
  $ hg diff -c .
  diff --git a/foo b/dir/baz
  rename from foo
  rename to dir/baz
  --- a/foo
  +++ b/dir/baz
  @@ -2,7 +2,7 @@
   0
   0
   0
  - 1
  + 1+
   2+
   3+
   4
  @@ -11,5 +11,5 @@
   7
   8
   9
  -10
  -11-
  +1+0
  +1+1-
  $ hg log -f -L relpath:baz,5:7 -p
  changeset:   9:6af29c3a778f
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo -> dir/baz; 1-1+
  
  diff --git a/foo b/dir/baz
  copy from foo
  copy to dir/baz
  --- a/foo
  +++ b/dir/baz
  @@ -2,7 +2,7 @@
   0
   0
   0
  - 1
  + 1+
   2+
   3+
   4
  
  changeset:   5:cfdf972b3971
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo: 3 -> 3+ and 11+ -> 11-; bar: a -> a+
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -4,7 +4,7 @@
   0
    1
   2+
  -3
  +3+
   4
   5
   6
  
  changeset:   4:eaec41c1a0c9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     11 -> 11+; leading space before "1"
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -2,7 +2,7 @@
   0
   0
   0
  -1
  + 1
   2+
   3
   4
  
  changeset:   2:63a884426fd0
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     2 -> 2+; added bar
  
  diff --git a/foo b/foo
  --- a/foo
  +++ b/foo
  @@ -3,6 +3,6 @@
   0
   0
   1
  -2
  +2+
   3
   4
  
  changeset:   0:5ae1f82b9a00
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     init
  
  diff --git a/foo b/foo
  new file mode 100644
  --- /dev/null
  +++ b/foo
  @@ -0,0 +1,5 @@
  +0
  +1
  +2
  +3
  +4
  

Binary files work but without diff hunks filtering.
(Checking w/ and w/o diff.git option.)

  >>> open('binary', 'w').write('this\nis\na\nbinary\0')
  $ hg add binary
  $ hg ci -m 'add a binary file' --quiet
  $ hg log -f -L binary,1:2 -p
  changeset:   10:c96381c229df
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a binary file
  
  diff --git a/dir/binary b/dir/binary
  new file mode 100644
  index e69de29bb2d1d6434b8b29ae775ad8c2e48c5391..c2e1fbed209fe919b3f189a6a31950e9adf61e45
  GIT binary patch
  literal 17
  Wc$_QA$SmdpqC~Ew%)G>+N(KNlNClYy
  
  
  $ hg log -f -L binary,1:2 -p --config diff.git=false
  changeset:   10:c96381c229df
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a binary file
  
  diff -r 6af29c3a778f -r c96381c229df dir/binary
  Binary file dir/binary has changed
  

Option --follow is required.

  $ hg log -L foo,5:7
  abort: --line-range requires --follow
  [255]

Non-exact pattern kinds are not allowed.

  $ cd ..
  $ hg log -f -L glob:*a*,1:2
  hg: parse error: line range pattern 'glob:*a*' must match exactly one file
  [255]

Graph log does work yet.

  $ hg log -f -L dir/baz,5:7 --graph
  abort: graph not supported with line range patterns
  [255]
