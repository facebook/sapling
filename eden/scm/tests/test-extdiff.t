#chg-compatible

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
  
  use external program to diff repository (or selected files)
  
      Show differences between revisions for the specified files, using the
      following program:
  
        'echo'
  
      When two revision arguments are given, then changes are shown between
      those revisions. If only one revision is specified then that revision is
      compared to the working directory, and, when no revisions are specified,
      the working directory files are compared to its parent.
  
  Options ([+] can be repeated):
  
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
      --patch               compare patches for two revisions
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)

  $ hg ci -d '0 0' -mtest1

  $ echo b >> a
  $ hg ci -d '1 0' -mtest2

Should diff cloned files directly:

  $ hg falabala -r 0:1
  diffing "*\\extdiff.*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

Specifying an empty revision should abort.

  $ hg extdiff -p diff --patch --rev 'ancestor()' --rev 1
  abort: empty revision on one side of range
  [255]

Test diff during merge:

  $ hg update -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c >> c
  $ hg add c
  $ hg ci -m "new branch" -d '1 0'
  $ hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

Should diff cloned file against wc file:

  $ hg falabala
  diffing "*\\extdiff.*\\a.2a13a4d2da36\\a" "*\\a\\a" (glob) (windows !)
  diffing */extdiff.*/a.2a13a4d2da36/a */a/a (glob) (no-windows !)
  [1]


Test --change option:

  $ hg ci -d '2 0' -mtest3

  $ hg falabala -c 1
  diffing "*\\extdiff.*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  diffing */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

Check diff are made from the first parent:

  $ hg falabala -c 3 || echo "diff-like tools yield a non-zero exit code"
  diffing "*\\extdiff.*\\a.2a13a4d2da36\\a" "a.46c0e4daeb72\\a" (glob) (windows !)
  diffing */extdiff.*/a.2a13a4d2da36/a a.46c0e4daeb72/a (glob) (no-windows !)
  diff-like tools yield a non-zero exit code

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

  $ hg --debug 4463a | grep '^running'
  running 'echo a-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running 'echo a-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)
  $ hg --debug 4463b | grep '^running'
  running 'echo b-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running 'echo b-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)
  $ hg --debug echo | grep '^running'
  running '*echo* "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running '*echo */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)

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

  $ hg --debug 4463b2 | grep '^running'
  running 'echo b2-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running 'echo b2-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)
  $ hg --debug 4463b3 | grep '^running'
  running 'echo b3-naked \'single quoted\' "double quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running 'echo b3-naked \'single quoted\' "double quoted" */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)
  $ hg --debug 4463b4 | grep '^running'
  running 'echo "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running 'echo */a $TESTTMP/a/a' in */extdiff.* (glob) (no-windows !)
  $ hg --debug 4463b4 --option b4-naked --option 'being quoted' | grep '^running'
  running 'echo b4-naked "being quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running "echo b4-naked 'being quoted' */a $TESTTMP/a/a" in */extdiff.* (glob) (no-windows !)
  $ hg --debug extdiff -p echo --option echo-naked --option 'being quoted' | grep '^running'
  running 'echo echo-naked "being quoted" "*\\a" "*\\a"' in */extdiff.* (glob) (windows !)
  running "echo echo-naked 'being quoted' */a $TESTTMP/a/a" in */extdiff.* (glob) (no-windows !)

  $ touch 'sp ace'
  $ hg add 'sp ace'
  $ hg ci -m 'sp ace'
  $ echo > 'sp ace'

Test pre-72a89cf86fcd backward compatibility with half-baked manual quoting

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > odd =
  > [merge-tools]
  > odd.diffargs = --foo='\$clabel' '\$clabel' "--bar=\$clabel" "\$clabel"
  > odd.executable = echo
  > EOF

  $ hg --debug odd | grep '^running'
  running '"*\\echo.exe" --foo="sp ace" "sp ace" --bar="sp ace" "sp ace"' in * (glob) (windows !)
  running "*/echo --foo='sp ace' 'sp ace' --bar='sp ace' 'sp ace'" in * (glob) (no-windows !)

Empty argument must be quoted

  $ cat <<EOF >> $HGRCPATH
  > [extdiff]
  > kdiff3 = echo
  > [merge-tools]
  > kdiff3.diffargs=--L1 \$plabel1 --L2 \$clabel \$parent \$child
  > EOF

  $ hg --debug kdiff3 -r0 | grep '^running'
  running 'echo --L1 "@0" --L2 "" a.8a5febb7f867 a' in * (glob) (windows !)
  running "echo --L1 '@0' --L2 '' a.8a5febb7f867 a" in * (glob) (no-windows !)


Test extdiff of multiple files in tmp dir:

  $ hg update -C 0 > /dev/null
  $ echo changed > a
  $ echo changed > b
#if execbit
  $ chmod +x b
#endif

Diff in working directory, before:

  $ hg diff --git
  diff --git a/a b/a
  --- a/a
  +++ b/a
  @@ -1,1 +1,1 @@
  -a
  +changed
  diff --git a/b b/b
  old mode 100644 (execbit !)
  new mode 100755 (execbit !)
  --- a/b
  +++ b/b
  @@ -1,1 +1,1 @@
  -b
  +changed


Edit with extdiff -p:

Prepare custom diff/edit tool:

  $ cat > 'diff tool.py' << EOT
  > #!$PYTHON
  > import time
  > time.sleep(1) # avoid unchanged-timestamp problems
  > file('a/a', 'ab').write('edited\n')
  > file('a/b', 'ab').write('edited\n')
  > EOT

#if execbit
  $ chmod +x 'diff tool.py'
#endif

will change to /tmp/extdiff.TMP and populate directories a.TMP and a
and start tool

#if windows
  $ cat > 'diff tool.bat' << EOF
  > @$PYTHON "`pwd`/diff tool.py"
  > EOF
  $ hg extdiff -p "`pwd`/diff tool.bat"
  [1]
#else
  $ hg extdiff -p "`pwd`/diff tool.py"
  [1]
#endif

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
  old mode 100644 (execbit !)
  new mode 100755 (execbit !)
  --- a/b
  +++ b/b
  @@ -1,1 +1,2 @@
  -b
  +changed
  +edited

Test extdiff with --option:

  $ hg extdiff -p echo -o this -c 1
  this "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  this */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

  $ hg falabala -o this -c 1
  diffing this "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  diffing this */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

Test extdiff's handling of options with spaces in them:

  $ hg edspace -c 1
  "name  <user@example.com>" "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  name  <user@example.com> */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

  $ hg extdiff -p echo -o "name  <user@example.com>" -c 1
  "name  <user@example.com>" "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  name  <user@example.com> */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

Test with revsets:

  $ hg extdif -p echo -c "rev(1)"
  "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

  $ hg extdif -p echo -r "0::1"
  "*\\a.8a5febb7f867\\a" "a.34eed99112ab\\a" (glob) (windows !)
  */extdiff.*/a.8a5febb7f867/a a.34eed99112ab/a (glob) (no-windows !)
  [1]

Fallback to merge-tools.tool.executable|regkey
  $ mkdir dir
  $ cat > 'dir/tool.sh' << 'EOF'
  > #!/bin/sh
  > # Mimic a tool that syncs all attrs, including mtime
  > cp $1/a $2/a
  > touch -r $1/a $2/a
  > chmod +x $2/a
  > echo "** custom diff **"
  > EOF
#if execbit
  $ chmod +x dir/tool.sh
#endif

Windows can't run *.sh directly, so create a shim executable that can be.
Without something executable, the next hg command will try to run `tl` instead
of $tool (and fail).
#if windows
  $ cat > dir/tool.bat <<EOF
  > @sh -c "`pwd`/dir/tool.sh %1 %2"
  > EOF
  $ tool=`pwd`/dir/tool.bat
#else
  $ tool=`pwd`/dir/tool.sh
#endif

  $ cat a
  changed
  edited
  $ hg --debug tl --config extdiff.tl= --config merge-tools.tl.executable=$tool
  making snapshot of 2 files from rev * (glob)
    a
    b
  making snapshot of 2 files from working directory
    a
    b
  running '$TESTTMP/a/dir/tool.bat a.* a' in */extdiff.* (glob) (windows !)
  running '$TESTTMP/a/dir/tool.sh a.* a' in */extdiff.* (glob) (no-windows !)
  ** custom diff **
  file changed while diffing. Overwriting: $TESTTMP/a/a (src: */extdiff.*/a/a) (glob)
  cleaning up temp directory
  [1]
  $ cat a
  a

#if execbit
  $ [ -x a ]

  $ cat > 'dir/tool.sh' << 'EOF'
  > #!/bin/sh
  > chmod -x $2/a
  > echo "** custom diff **"
  > EOF

  $ hg --debug tl --config extdiff.tl= --config merge-tools.tl.executable=$tool
  making snapshot of 2 files from rev * (glob)
    a
    b
  making snapshot of 2 files from working directory
    a
    b
  running '$TESTTMP/a/dir/tool.sh a.* a' in */extdiff.* (glob)
  ** custom diff **
  file changed while diffing. Overwriting: $TESTTMP/a/a (src: */extdiff.*/a/a) (glob)
  cleaning up temp directory
  [1]

  $ [ -x a ]
  [1]
#endif

  $ cd ..

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

Test handling of non-ASCII paths in generated docstrings (issue5301)

  >>> open("u", "w").write("\xa5\xa5")
  $ U=`cat u`

  $ HGPLAIN=1 hg --config hgext.extdiff= --config extdiff.cmd.td=hi help -k xyzzy
  abort: no matches
  (try 'hg help' for a list of topics)
  [255]

  $ HGPLAIN=1 hg --config hgext.extdiff= --config extdiff.cmd.td=hi help td > /dev/null
