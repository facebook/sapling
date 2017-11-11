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

Parsing of early options should stop at "--":

  $ hg cat -- --config=hooks.pre-cat=false
  --config=hooks.pre-cat=false: no such file in rev cb9a9f314b8b
  [1]
  $ hg cat -- --debugger
  --debugger: no such file in rev cb9a9f314b8b
  [1]

Unparsable form of early options:

  $ hg cat --debugg
  abort: option --debugger may not be abbreviated!
  [255]

Parsing failure of early options should be detected before executing the
command:

  $ hg log -b '--config=hooks.pre-log=false' default
  abort: option --config may not be abbreviated!
  [255]
  $ hg log -b -R. default
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg log --cwd .. -b --cwd=. default
  abort: option --cwd may not be abbreviated!
  [255]

However, we can't prevent it from loading extensions and configs:

  $ cat <<EOF > bad.py
  > raise Exception('bad')
  > EOF
  $ hg log -b '--config=extensions.bad=bad.py' default
  *** failed to import extension bad from bad.py: bad
  abort: option --config may not be abbreviated!
  [255]

  $ mkdir -p badrepo/.hg
  $ echo 'invalid-syntax' > badrepo/.hg/hgrc
  $ hg log -b -Rbadrepo default
  hg: parse error at badrepo/.hg/hgrc:1: invalid-syntax
  [255]

  $ hg log -b --cwd=inexistent default
  abort: No such file or directory: 'inexistent'
  [255]

  $ hg log -b '--config=ui.traceback=yes' 2>&1 | grep '^Traceback'
  Traceback (most recent call last):
  $ hg log -b '--config=profiling.enabled=yes' 2>&1 | grep -i sample
  Sample count: .*|No samples recorded\. (re)

Early options can't be specified in [aliases] and [defaults] because they are
applied before the command name is resolved:

  $ hg log -b '--config=alias.log=log --config=hooks.pre-log=false'
  hg log: option -b not recognized
  error in definition for alias 'log': --config may only be given on the command
  line
  [255]

  $ hg log -b '--config=defaults.log=--config=hooks.pre-log=false'
  abort: option --config may not be abbreviated!
  [255]

Shell aliases bypass any command parsing rules but for the early one:

  $ hg log -b '--config=alias.log=!echo howdy'
  howdy

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
