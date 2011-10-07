  $ echo "[extensions]" >> $HGRCPATH
  $ echo "extdiff=" >> $HGRCPATH

  $ hg init a
  $ cd a
  $ echo a > a
  $ echo b > b
  $ hg add
  adding a
  adding b

Should diff cloned directories:

  $ hg extdiff -o -r $opt
  Only in a: a
  Only in a: b
  [1]

  $ echo "[extdiff]" >> $HGRCPATH
  $ echo "cmd.falabala=echo" >> $HGRCPATH
  $ echo "opts.falabala=diffing" >> $HGRCPATH

  $ hg falabala
  diffing a.000000000000 a
  [1]

  $ hg help falabala
  hg falabala [OPTION]... [FILE]...
  
  use 'echo' to diff repository (or selected files)
  
      Show differences between revisions for the specified files, using the
      'echo' program.
  
      When two revision arguments are given, then changes are shown between
      those revisions. If only one revision is specified then that revision is
      compared to the working directory, and, when no revisions are specified,
      the working directory files are compared to its parent.
  
  options:
  
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  [+] marked option can be specified multiple times
  
  use "hg -v help falabala" to show more info

  $ hg ci -d '0 0' -mtest1

  $ echo b >> a
  $ hg ci -d '1 0' -mtest2

Should diff cloned files directly:

  $ hg falabala -r 0:1
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

Test diff during merge:

  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg add c
  $ hg ci -m "new branch" -d '1 0'
  created new head
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Should diff cloned file against wc file:

  $ hg falabala
  diffing */extdiff.*/a.2a13a4d2da36/a */a/a (glob)
  [1]


Test --change option:

  $ hg ci -d '2 0' -mtest3
  $ hg falabala -c 1
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

Check diff are made from the first parent:

  $ hg falabala -c 3 || echo "diff-like tools yield a non-zero exit code"
  diffing */extdiff.*/a.2a13a4d2da36/a a.46c0e4daeb72/a (glob)
  diff-like tools yield a non-zero exit code

Test extdiff of multiple files in tmp dir:

  $ hg update -C 0 > /dev/null
  $ echo changed > a
  $ echo changed > b
  $ chmod +x b

Diff in working directory, before:

  $ hg diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +changed
  diff --git a/b b/b
  old mode 100644
  new mode 100755
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -b
  +changed


Edit with extdiff -p:

Prepare custom diff/edit tool:

  $ cat > 'diff tool.py' << EOT
  > #!/usr/bin/env python
  > import time
  > time.sleep(1) # avoid unchanged-timestamp problems
  > file('a/a', 'ab').write('edited\n')
  > file('a/b', 'ab').write('edited\n')
  > EOT

  $ chmod +x 'diff tool.py'

will change to /tmp/extdiff.TMP and populate directories a.TMP and a
and start tool

  $ hg extdiff -p "`pwd`/diff tool.py"
  [1]

Diff in working directory, after:

  $ hg diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,2 @@
  -a
  +changed
  +edited
  diff --git a/b b/b
  old mode 100644
  new mode 100755
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
  -b
  +changed
  +edited

Test extdiff with --option:

  $ hg extdiff -p echo -o this -c 1
  this */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

  $ hg falabala -o this -c 1
  diffing this */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

Test with revsets:

  $ hg extdif -p echo -c "rev(1)"
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

  $ hg extdif -p echo -r "0::1"
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

  $ cd ..

Test symlinks handling (issue1909)

  $ hg init testsymlinks
  $ cd testsymlinks
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ ln -s missing linka
  $ hg add linka
  $ hg falabala -r 0 --traceback
  diffing testsymlinks.07f494440405 testsymlinks
  [1]
  $ cd ..
