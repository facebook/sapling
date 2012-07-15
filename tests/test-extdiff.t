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

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > cmd.falabala = echo
  > opts.falabala = diffing
  > cmd.edspace = echo
  > opts.edspace = "name  <user@example.com>"
  > EOF

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
  
  options ([+] can be repeated):
  
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
  
  (some details hidden, use --verbose to show complete help)

  $ hg ci -d '0 0' -mtest1

  $ echo b >> a
  $ hg ci -d '1 0' -mtest2

Should diff cloned files directly:

#if windows
  $ hg falabala -r 0:1
  diffing "*\\extdiff.*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob)
  [1]
#else
  $ hg falabala -r 0:1
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]
#endif

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

#if windows
  $ hg falabala
  diffing "*\\extdiff.*\\a.2a13a4d2da36\\a" "*\\a\\a" (glob)
  [1]
#else
  $ hg falabala
  diffing */extdiff.*/a.2a13a4d2da36/a */a/a (glob)
  [1]
#endif


Test --change option:

  $ hg ci -d '2 0' -mtest3
#if windows
  $ hg falabala -c 1
  diffing "*\\extdiff.*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob)
  [1]
#else
  $ hg falabala -c 1
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]
#endif

Check diff are made from the first parent:

#if windows
  $ hg falabala -c 3 || echo "diff-like tools yield a non-zero exit code"
  diffing "*\\extdiff.*\\a.2a13a4d2da36\\a" "a.46c0e4daeb72\\a" (glob)
  diff-like tools yield a non-zero exit code
#else
  $ hg falabala -c 3 || echo "diff-like tools yield a non-zero exit code"
  diffing */extdiff.*/a.2a13a4d2da36/a a.46c0e4daeb72/a (glob)
  diff-like tools yield a non-zero exit code
#endif

issue4463: usage of command line configuration without additional quoting

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > cmd.4463a = echo
  > opts.4463a = a-naked 'single quoted' "double quoted"
  > 4463b = echo b-naked 'single quoted' "double quoted"
  > echo =
  > EOF
  $ hg update -q -C 0
  $ echo a >> a
#if windows
  $ hg --debug 4463a | grep '^running'
  running 'echo a-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug 4463b | grep '^running'
  running 'echo b-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug echo | grep '^running'
  running '*echo* "*\\a" "*\\a"' in */extdiff.* (glob)
#else
  $ hg --debug 4463a | grep '^running'
  running 'echo a-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob)
  $ hg --debug 4463b | grep '^running'
  running 'echo b-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob)
  $ hg --debug echo | grep '^running'
  running '*echo */a $TESTTMP/a/a' in */extdiff.* (glob)
#endif

(getting options from other than extdiff section)

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > # using diff-tools diffargs
  > 4463b2 = echo
  > # using merge-tools diffargs
  > 4463b3 = echo
  > # no diffargs
  > 4463b4 = echo
  > [diff-tools]
  > 4463b2.diffargs = b2-naked 'single quoted' "double quoted"
  > [merge-tools]
  > 4463b3.diffargs = b3-naked 'single quoted' "double quoted"
  > EOF
#if windows
  $ hg --debug 4463b2 | grep '^running'
  running 'echo b2-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug 4463b3 | grep '^running'
  running 'echo b3-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug 4463b4 | grep '^running'
  running 'echo "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug 4463b4 --option b4-naked --option 'being quoted' | grep '^running'
  running 'echo b4-naked "being quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
  $ hg --debug extdiff -p echo --option echo-naked --option 'being quoted' | grep '^running'
  running 'echo echo-naked "being quoted" "*\\a" "*\\a"' in */extdiff.* (glob)
#else
  $ hg --debug 4463b2 | grep '^running'
  running 'echo b2-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob)
  $ hg --debug 4463b3 | grep '^running'
  running 'echo b3-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob)
  $ hg --debug 4463b4 | grep '^running'
  running 'echo */a $TESTTMP/a/a' in */extdiff.* (glob)
  $ hg --debug 4463b4 --option b4-naked --option 'being quoted' | grep '^running'
  running "echo b4-naked 'being quoted' */a $TESTTMP/a/a" in */extdiff.* (glob)
  $ hg --debug extdiff -p echo --option echo-naked --option 'being quoted' | grep '^running'
  running "echo echo-naked 'being quoted' */a $TESTTMP/a/a" in */extdiff.* (glob)
#endif

  $ touch 'sp ace'
  $ hg add 'sp ace'
  $ hg ci -m 'sp ace'
  created new head
  $ echo > 'sp ace'

Test pre-72a89cf86fcd backward compatibility with half-baked manual quoting

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > odd =
  > [merge-tools]
  > odd.diffargs = --foo='\$clabel' '\$clabel' "--bar=\$clabel" "\$clabel"
  > odd.executable = echo
  > EOF
#if windows
TODO
#else
  $ hg --debug odd | grep '^running'
  running "*/echo --foo='sp ace' 'sp ace' --bar='sp ace' 'sp ace'" in * (glob)
#endif

Empty argument must be quoted

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > kdiff3 = echo
  > [merge-tools]
  > kdiff3.diffargs=--L1 \$plabel1 --L2 \$clabel \$parent \$child
  > EOF
#if windows
  $ hg --debug kdiff3 -r0 | grep '^running'
  running 'echo --L1 "@0" --L2 "" a.8a5febb7f867 a' in * (glob)
#else
  $ hg --debug kdiff3 -r0 | grep '^running'
  running "echo --L1 '@0' --L2 '' a.8a5febb7f867 a" in * (glob)
#endif

#if execbit

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

Test extdiff's handling of options with spaces in them:

  $ hg edspace -c 1
  name  <user@example.com> */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

  $ hg extdiff -p echo -o "name  <user@example.com>" -c 1
  name  <user@example.com> */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

Test with revsets:

  $ hg extdif -p echo -c "rev(1)"
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

  $ hg extdif -p echo -r "0::1"
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob)
  [1]

Fallback to merge-tools.tool.executable|regkey
  $ mkdir dir
  $ cat > 'dir/tool.sh' << EOF
  > #!/bin/sh
  > echo "** custom diff **"
  > EOF
  $ chmod +x dir/tool.sh
  $ tool=`pwd`/dir/tool.sh
  $ hg --debug tl --config extdiff.tl= --config merge-tools.tl.executable=$tool
  making snapshot of 2 files from rev * (glob)
    a
    b
  making snapshot of 2 files from working directory
    a
    b
  running '$TESTTMP/a/dir/tool.sh a.* a' in */extdiff.* (glob)
  ** custom diff **
  cleaning up temp directory
  [1]

  $ cd ..

#endif

#if symlink

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

#endif
