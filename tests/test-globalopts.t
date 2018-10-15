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
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets b6c483daf290
  (run 'hg heads' to see heads, 'hg merge' to merge)
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cd ..

Testing -R/--repository:

  $ hg -R a tip
  changeset:   0:8580ff50825a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  $ hg --repository b tip
  changeset:   0:b6c483daf290
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  

-R with a URL:

  $ hg -R file:a identify
  8580ff50825a tip
  $ hg -R file://localhost/`pwd`/a/ identify
  8580ff50825a tip

-R with path aliases:

  $ cd c
  $ hg -R default identify
  8580ff50825a tip
  $ hg -R relative identify
  8580ff50825a tip
  $ echo '[paths]' >> $HGRCPATH
  $ echo 'relativetohome = a' >> $HGRCPATH
  $ HOME=`pwd`/../ hg -R relativetohome identify
  8580ff50825a tip
  $ cd ..

#if no-outer-repo

Implicit -R:

  $ hg ann a/a
  0: a
  $ hg ann a/a a/a
  0: a
  $ hg ann a/a b/b
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]
  $ hg -R b ann a/a
  abort: a/a not under root '$TESTTMP/b'
  (consider using '--cwd b')
  [255]
  $ hg log
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]

#endif

Abbreviation of long option:

  $ hg --repo c tip
  changeset:   1:b6c483daf290
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  

earlygetopt with duplicate options (36d23de02da1):

  $ hg --cwd a --cwd b --cwd c tip
  changeset:   1:b6c483daf290
  tag:         tip
  parent:      -1:000000000000
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  
  $ hg --repo c --repository b -R a tip
  changeset:   0:8580ff50825a
  tag:         tip
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
  tag:         tip
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
  tag:         tip
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
  tag:         tip
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
  tag:         tip
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    1:23226e7a252cacdc2d99e4fbdc3653441056de49
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
  manifest:    0:a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files+:      a
  extra:       branch=default
  description:
  a
  
  

Testing --traceback:

#if no-chg
  $ hg --cwd c --config x --traceback id 2>&1 | grep -i 'traceback'
  Traceback (most recent call last):
#else
Traceback for '--config' errors not supported with chg.
  $ hg --cwd c --config x --traceback id 2>&1 | grep -i 'traceback'
  [1]
#endif

Testing --time:

  $ hg --cwd a --time id
  8580ff50825a tip
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
  
  Create repositories:
  
   clone         make a copy of an existing repository
   init          create a new repository in the given directory
  
  Examine files in your current checkout:
  
   grep          search revision history for a pattern in specified files
   status        show changed files in the working directory
  
  Work on your current checkout:
  
   add           add the specified files on the next commit
   copy          mark files as copied for the next commit
   remove        remove the specified files on the next commit
   rename        rename files; equivalent of copy + remove
  
  Commit changes and modify commits:
  
   commit        commit the specified files or all outstanding changes
  
  Look at commits and commit history:
  
   diff          diff repository (or selected files)
   log           show revision history of entire repository or files
   show          show revision in detail
  
  Checkout other commits:
  
   update        update working directory (or switch revisions)
  
  Rearrange commits:
  
   graft         copy changes from other branches onto the current branch
  
  Exchange commits with a server:
  
   pull          pull changes from the specified source
   push          push changes to the specified destination
  
  Additional help topics:
  
   glossary      glossary
   patterns      file name patterns
   revisions     specifying revisions
   templating    template usage



  $ hg --help
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
  Create repositories:
  
   clone         make a copy of an existing repository
   init          create a new repository in the given directory
  
  Examine files in your current checkout:
  
   grep          search revision history for a pattern in specified files
   status        show changed files in the working directory
  
  Work on your current checkout:
  
   add           add the specified files on the next commit
   copy          mark files as copied for the next commit
   remove        remove the specified files on the next commit
   rename        rename files; equivalent of copy + remove
  
  Commit changes and modify commits:
  
   commit        commit the specified files or all outstanding changes
  
  Look at commits and commit history:
  
   diff          diff repository (or selected files)
   log           show revision history of entire repository or files
   show          show revision in detail
  
  Checkout other commits:
  
   update        update working directory (or switch revisions)
  
  Rearrange commits:
  
   graft         copy changes from other branches onto the current branch
  
  Exchange commits with a server:
  
   pull          pull changes from the specified source
   push          push changes to the specified destination
  
  Additional help topics:
  
   glossary      glossary
   patterns      file name patterns
   revisions     specifying revisions
   templating    template usage

Not tested: --debugger

