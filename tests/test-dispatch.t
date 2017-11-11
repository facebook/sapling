test command parsing and dispatch

  $ hg init a
  $ cd a

Redundant options used to crash (issue436):
  $ hg -v log -v
  $ hg -v log -v x

  $ echo a > a
  $ hg ci -Ama
  adding a

Missing arg:

  $ hg cat
  hg cat: invalid arguments
  hg cat [OPTION]... FILE...
  
  output the current or given revision of files
  
  options ([+] can be repeated):
  
   -o --output FORMAT       print output to file with formatted name
   -r --rev REV             print the given revision
      --decode              apply any matching decode filter
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (use 'hg cat -h' to show more help)
  [255]

Missing parameter for early option:

  $ hg log -R 2>&1 | grep 'hg log'
  hg log: option -R requires argument
  hg log [OPTION]... [FILE]
  (use 'hg log -h' to show more help)

  $ hg log -R -- 2>&1 | grep 'hg log'
  hg log: option -R requires argument
  hg log [OPTION]... [FILE]
  (use 'hg log -h' to show more help)

[defaults]

  $ hg cat a
  a
  $ cat >> $HGRCPATH <<EOF
  > [defaults]
  > cat = -r null
  > EOF
  $ hg cat a
  a: no such file in rev 000000000000
  [1]

  $ cd "$TESTTMP"

OSError "No such file or directory" / "The system cannot find the path
specified" should include filename even when it is empty

  $ hg -R a archive ''
  abort: *: '' (glob)
  [255]

#if no-outer-repo

No repo:

  $ hg cat
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]

#endif

#if rmcwd

Current directory removed:

  $ mkdir $TESTTMP/repo1
  $ cd $TESTTMP/repo1
  $ rm -rf $TESTTMP/repo1

The output could be one of the following and something else:
 chg: abort: failed to getcwd (errno = *) (glob)
 abort: error getting current working directory: * (glob)
 sh: 0: getcwd() failed: No such file or directory
Since the exact behavior depends on the shell, only check it returns non-zero.
  $ HGDEMANDIMPORT=disable hg version -q 2>/dev/null || false
  [1]

#endif
