test command parsing and dispatch

  $ "$TESTDIR/hghave" no-outer-repo || exit 80

  $ dir=`pwd`

  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -Ama
  adding a

Missing arg:

  $ hg cat
  hg cat: invalid arguments
  hg cat [OPTION]... FILE...
  
  output the current or given revision of files
  
      Print the specified files as they were at the given revision. If no
      revision is given, the parent of the working directory is used, or tip if
      no revision is checked out.
  
      Output may be to a file, in which case the name of the file is given using
      a format string. The formatting rules are the same as for the export
      command, with the following additions:
  
      "%s"  basename of file being printed
      "%d"  dirname of file being printed, or '.' if in repository root
      "%p"  root-relative path name of file being printed
  
      Returns 0 on success.
  
  options:
  
   -o --output FORMAT        print output to file with formatted name
   -r --rev REV              print the given revision
      --decode               apply any matching decode filter
   -I --include PATTERN [+]  include names matching the given patterns
   -X --exclude PATTERN [+]  exclude names matching the given patterns
  
  [+] marked option can be specified multiple times
  
  use "hg -v help cat" to show global options
  [255]

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

No repo:

  $ cd $dir
  $ hg cat
  abort: There is no Mercurial repository here (.hg not found)!
  [255]

