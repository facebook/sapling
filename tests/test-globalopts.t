  $ "$TESTDIR/hghave" no-outer-repo || exit 80

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

Implicit -R:

  $ hg ann a/a
  0: a
  $ hg ann a/a a/a
  0: a
  $ hg ann a/a b/b
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]
  $ hg -R b ann a/a
  abort: a/a not under root
  [255]
  $ hg log
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]

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

  $ hg --confi "foo.bar=baz"
  abort: option --config may not be abbreviated!
  [255]
  $ hg --cw a tip
  abort: option --cwd may not be abbreviated!
  [255]
  $ hg --rep a tip
  abort: Option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg --repositor a tip
  abort: Option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg -qR a tip
  abort: Option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg -qRa tip
  abort: Option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
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

  $ hg --cwd c --config x --traceback id 2>&1 | grep -i 'traceback'
  Traceback (most recent call last):

Testing --time:

  $ hg --cwd a --time id
  8580ff50825a tip
  Time: real * (glob)

Testing --version:

  $ hg --version -q
  Mercurial Distributed SCM * (glob)

Testing -h/--help:

  $ hg -h
  Mercurial Distributed SCM
  
  list of commands:
  
   add          add the specified files on the next commit
   addremove    add all new files, delete all missing files
   annotate     show changeset information by line for each file
   archive      create an unversioned archive of a repository revision
   backout      reverse effect of earlier changeset
   bisect       subdivision search of changesets
   bookmarks    track a line of development with movable markers
   branch       set or show the current branch name
   branches     list repository named branches
   bundle       create a changegroup file
   cat          output the current or given revision of files
   clone        make a copy of an existing repository
   commit       commit the specified files or all outstanding changes
   copy         mark files as copied for the next commit
   diff         diff repository (or selected files)
   export       dump the header and diffs for one or more changesets
   forget       forget the specified files on the next commit
   graft        copy changes from other branches onto the current branch
   grep         search for a pattern in specified files and revisions
   heads        show current repository heads or show branch heads
   help         show help for a given topic or a help overview
   identify     identify the working copy or specified revision
   import       import an ordered set of patches
   incoming     show new changesets found in source
   init         create a new repository in the given directory
   locate       locate files matching specific patterns
   log          show revision history of entire repository or files
   manifest     output the current or given revision of the project manifest
   merge        merge working directory with another revision
   outgoing     show changesets not found in the destination
   parents      show the parents of the working directory or revision
   paths        show aliases for remote repositories
   pull         pull changes from the specified source
   push         push changes to the specified destination
   recover      roll back an interrupted transaction
   remove       remove the specified files on the next commit
   rename       rename files; equivalent of copy + remove
   resolve      redo merges or set/view the merge status of files
   revert       restore files to their checkout state
   rollback     roll back the last transaction (dangerous)
   root         print the root (top) of the current working directory
   serve        start stand-alone webserver
   showconfig   show combined config settings from all hgrc files
   status       show changed files in the working directory
   summary      summarize working directory state
   tag          add one or more tags for the current or given revision
   tags         list repository tags
   tip          show the tip revision
   unbundle     apply one or more changegroup files
   update       update working directory (or switch revisions)
   verify       verify the integrity of the repository
   version      output version and copyright information
  
  additional help topics:
  
   config       Configuration Files
   dates        Date Formats
   diffs        Diff Formats
   environment  Environment Variables
   extensions   Using additional features
   filesets     Specifying File Sets
   glossary     Glossary
   hgignore     syntax for Mercurial ignore files
   hgweb        Configuring hgweb
   merge-tools  Merge Tools
   multirevs    Specifying Multiple Revisions
   patterns     File Name Patterns
   revisions    Specifying Single Revisions
   revsets      Specifying Revision Sets
   subrepos     Subrepositories
   templating   Template Usage
   urls         URL Paths
  
  use "hg -v help" to show builtin aliases and global options



  $ hg --help
  Mercurial Distributed SCM
  
  list of commands:
  
   add          add the specified files on the next commit
   addremove    add all new files, delete all missing files
   annotate     show changeset information by line for each file
   archive      create an unversioned archive of a repository revision
   backout      reverse effect of earlier changeset
   bisect       subdivision search of changesets
   bookmarks    track a line of development with movable markers
   branch       set or show the current branch name
   branches     list repository named branches
   bundle       create a changegroup file
   cat          output the current or given revision of files
   clone        make a copy of an existing repository
   commit       commit the specified files or all outstanding changes
   copy         mark files as copied for the next commit
   diff         diff repository (or selected files)
   export       dump the header and diffs for one or more changesets
   forget       forget the specified files on the next commit
   graft        copy changes from other branches onto the current branch
   grep         search for a pattern in specified files and revisions
   heads        show current repository heads or show branch heads
   help         show help for a given topic or a help overview
   identify     identify the working copy or specified revision
   import       import an ordered set of patches
   incoming     show new changesets found in source
   init         create a new repository in the given directory
   locate       locate files matching specific patterns
   log          show revision history of entire repository or files
   manifest     output the current or given revision of the project manifest
   merge        merge working directory with another revision
   outgoing     show changesets not found in the destination
   parents      show the parents of the working directory or revision
   paths        show aliases for remote repositories
   pull         pull changes from the specified source
   push         push changes to the specified destination
   recover      roll back an interrupted transaction
   remove       remove the specified files on the next commit
   rename       rename files; equivalent of copy + remove
   resolve      redo merges or set/view the merge status of files
   revert       restore files to their checkout state
   rollback     roll back the last transaction (dangerous)
   root         print the root (top) of the current working directory
   serve        start stand-alone webserver
   showconfig   show combined config settings from all hgrc files
   status       show changed files in the working directory
   summary      summarize working directory state
   tag          add one or more tags for the current or given revision
   tags         list repository tags
   tip          show the tip revision
   unbundle     apply one or more changegroup files
   update       update working directory (or switch revisions)
   verify       verify the integrity of the repository
   version      output version and copyright information
  
  additional help topics:
  
   config       Configuration Files
   dates        Date Formats
   diffs        Diff Formats
   environment  Environment Variables
   extensions   Using additional features
   filesets     Specifying File Sets
   glossary     Glossary
   hgignore     syntax for Mercurial ignore files
   hgweb        Configuring hgweb
   merge-tools  Merge Tools
   multirevs    Specifying Multiple Revisions
   patterns     File Name Patterns
   revisions    Specifying Single Revisions
   revsets      Specifying Revision Sets
   subrepos     Subrepositories
   templating   Template Usage
   urls         URL Paths
  
  use "hg -v help" to show builtin aliases and global options

Not tested: --debugger

