#chg-compatible

  $ disable treemanifest
  $ hg init a
  $ cd a
  $ echo a > a
  $ hg ci -A -d'1 0' -m a
  adding a

  $ cd ..

  $ hg init b
  $ cd b
  $ echo b > b
  $ hg ci -A -d'1 0' -m b
  adding b

  $ cd ..

  $ hg clone a c
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd c
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > relative = ../a
  > EOF
  $ hg pull -f ../b
  pulling from ../b
  searching for changes
  warning: repository is unrelated
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cd ..

Testing -R/--repository:

  $ hg -R a tip
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  $ hg --repository b tip
  changeset:   0:b6c483daf290
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  
#if no-outer-repo

Implicit -R:

  $ hg ann a/a
  0: a
  $ hg ann a/a a/a
  0: a
  $ hg ann a/a b/b
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]
  $ hg -R b ann a/a
  abort: a/a not under root '$TESTTMP/b'
  (consider using '--cwd b')
  [255]
  $ hg log
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

#endif

Abbreviation of long option:

  $ hg --repo c tip
  changeset:   1:b6c483daf290
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  

earlygetopt with duplicate options (36d23de02da1):

  $ hg --cwd a --cwd b --cwd c tip
  changeset:   1:b6c483daf290
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  
  $ hg --repo c --repository b -R a tip
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

earlygetopt short option without following space:

  $ hg -q -Rb tip
  0:b6c483daf290

earlygetopt with illegal abbreviations:

  $ hg --configfi "foo.bar=baz"
  abort: option --configfile may not be abbreviated!
  [255]
  $ hg --cw a tip
  abort: option --cwd may not be abbreviated!
  [255]
  $ hg --rep a tip
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg --repositor a tip
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg -qR a tip
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg -qRa tip
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]

Testing --cwd:

  $ hg --cwd a parents
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

Testing -y/--noninteractive - just be sure it is parsed:

  $ hg --cwd a tip -q --noninteractive
  0:8580ff50825a
  $ hg --cwd a tip -q -y
  0:8580ff50825a

Testing -q/--quiet:

  $ hg -R a -q tip
  0:8580ff50825a
  $ hg -R b -q tip
  0:b6c483daf290
  $ hg -R c --quiet parents
  0:8580ff50825a
  1:b6c483daf290

Testing -v/--verbose:

  $ hg --cwd c head -v
  changeset:   1:b6c483daf290
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       b
  description:
  b
  
  
  changeset:   0:8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  
  $ hg --cwd b tip --verbose
  changeset:   0:b6c483daf290
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       b
  description:
  b
  
  

Testing --config:

  $ hg --cwd c --config paths.quuxfoo=bar paths | grep quuxfoo > /dev/null && echo quuxfoo
  quuxfoo
  $ hg --cwd c --config '' tip -q
  abort: malformed --config option: '' (use --config section.name=value)
  [255]
  $ hg --cwd c --config a.b tip -q
  abort: malformed --config option: 'a.b' (use --config section.name=value)
  [255]
  $ hg --cwd c --config a tip -q
  abort: malformed --config option: 'a' (use --config section.name=value)
  [255]
  $ hg --cwd c --config a.= tip -q
  abort: malformed --config option: 'a.=' (use --config section.name=value)
  [255]
  $ hg --cwd c --config .b= tip -q
  abort: malformed --config option: '.b=' (use --config section.name=value)
  [255]

Testing --debug:

  $ hg --cwd c log --debug
  changeset:   1:b6c483daf2907ce5825c0bb50f5716226281cc1a
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    23226e7a252cacdc2d99e4fbdc3653441056de49
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files+:      b
  extra:       branch=default
  description:
  b
  
  
  changeset:   0:8580ff50825a50c8f716709acdf8de0deddcd6ab
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files+:      a
  extra:       branch=default
  description:
  a
  
  

Testing --traceback (this does not work with the Rust code path):

#if false
  $ hg --cwd c --config x --traceback id 2>&1 | grep -i 'traceback'
  Traceback (most recent call last):
#endif

Testing --time:

  $ hg --cwd a --time id
  8580ff50825a
  time: real * (glob)

Testing --version:

  $ hg --version -q
  Mercurial Distributed SCM * (glob)

hide outer repo
  $ hg init

Testing -h/--help:

  $ hg -h
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
  Get the latest commits from the server:
  
   pull          pull changes from the specified source
  
  View commits:
  
   show          show commit in detail
   diff          show differences between commits
  
  Check out a commit:
  
   checkout      check out a specific commit
  
  Work with your checkout:
  
   status        list files with pending changes
   add           start tracking the specified files
   remove        delete the specified tracked files
   forget        stop tracking the specified files
   revert        change the specified files to match a commit
  
  Commit changes and modify commits:
  
   commit        save all pending changes or specified files in a new commit
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   grep          search for a pattern in tracked files in the working directory
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates



  $ hg --help
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
  Get the latest commits from the server:
  
   pull          pull changes from the specified source
  
  View commits:
  
   show          show commit in detail
   diff          show differences between commits
  
  Check out a commit:
  
   checkout      check out a specific commit
  
  Work with your checkout:
  
   status        list files with pending changes
   add           start tracking the specified files
   remove        delete the specified tracked files
   forget        stop tracking the specified files
   revert        change the specified files to match a commit
  
  Commit changes and modify commits:
  
   commit        save all pending changes or specified files in a new commit
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   grep          search for a pattern in tracked files in the working directory
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

Not tested: --debugger

