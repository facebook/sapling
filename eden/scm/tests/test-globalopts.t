
#require no-eden

  $ setconfig devel.collapse-traceback=true

  $ newclientrepo server
  $ newclientrepo a server_server
  $ echo a > a
  $ sl ci -A -d'1 0' -m a
  adding a
  $ sl push -q -r . --to master_a --create

  $ newclientrepo b server_server
  $ echo b > b
  $ sl ci -A -d'1 0' -m b
  adding b
  $ sl push -q -r . --to master_b --create --force

  $ newclientrepo c server_server master_a
  $ cat >> .sl/config <<EOF
  > [paths]
  > relative = ../a
  > EOF
  $ sl pull -f test:server_server -B master_b
  pulling from test:server_server
  searching for changes
  $ sl merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cd ..

Testing -R/--repository:

  $ sl -R a tip
  commit:      8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  
  $ sl --repository b tip
  commit:      b6c483daf290
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  
#if no-outer-repo

Implicit -R:

  $ sl ann a/a
  0: a
  $ sl ann a/a a/a
  0: a
  $ sl ann a/a b/b
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]
  $ sl -R b ann a/a
  abort: a/a not under root '$TESTTMP/b'
  (consider using '--cwd b')
  [255]
  $ sl log
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

#endif

Abbreviation of long option:

  $ sl --repo c tip
  commit:      b6c483daf290
  bookmark:    remote/master_b
  hoistedname: master_b
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  

earlygetopt with duplicate options (36d23de02da1):

  $ sl --cwd a --cwd b --cwd c tip
  commit:      b6c483daf290
  bookmark:    remote/master_b
  hoistedname: master_b
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     b
  
  $ sl --repo c --repository b -R a tip
  commit:      8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

earlygetopt short option without following space:

  $ sl -q -Rb tip
  b6c483daf290

earlygetopt with illegal abbreviations:

  $ sl --configfi "foo.bar=baz"
  abort: option --configfile may not be abbreviated or used in aliases
  [255]
  $ sl --cw a tip
  abort: option --cwd may not be abbreviated or used in aliases
  [255]
  $ sl --rep a tip
  abort: option -R must appear alone, and --repository may not be abbreviated or used in aliases
  [255]
  $ sl --repositor a tip
  abort: option -R must appear alone, and --repository may not be abbreviated or used in aliases
  [255]
  $ sl -qR a tip
  abort: option -R must appear alone, and --repository may not be abbreviated or used in aliases
  [255]
  $ sl -qRa tip
  abort: option -R must appear alone, and --repository may not be abbreviated or used in aliases
  [255]

Testing --cwd:

  $ sl --cwd a parents
  commit:      8580ff50825a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     a
  

Testing -y/--noninteractive - just be sure it is parsed:

  $ sl --cwd a tip -q --noninteractive
  8580ff50825a
  $ sl --cwd a tip -q -y
  8580ff50825a

Testing -q/--quiet:

  $ sl -R a -q tip
  8580ff50825a
  $ sl -R b -q tip
  b6c483daf290
  $ sl -R c --quiet parents
  8580ff50825a
  b6c483daf290

  $ sl config ui.quiet -q --config config.use-rust=true
  true
  $ sl config ui.quiet --quiet --config config.use-rust=true
  true
  $ sl config ui.quiet --quie --config config.use-rust=true
  true
  $ sl config ui.quiet -q --config config.use-rust=false
  True
  $ sl config ui.quiet --quiet --config config.use-rust=false
  True
  $ sl config ui.quiet --quie --config config.use-rust=false
  True

Testing -v/--verbose:

  $ sl --cwd c head -v
  commit:      b6c483daf290
  bookmark:    remote/master_b
  hoistedname: master_b
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       b
  description:
  b
  
  
  commit:      8580ff50825a
  bookmark:    remote/master_a
  hoistedname: master_a
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       a
  description:
  a
  
  
  $ sl --cwd b tip --verbose
  commit:      b6c483daf290
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files:       b
  description:
  b
  
  

Testing --config:

  $ sl --cwd c --config paths.quuxfoo=bar paths | grep quuxfoo > /dev/null && echo quuxfoo
  quuxfoo
  $ sl --cwd c --config '' tip -q
  sl: parse errors: malformed --config option: '' (use --config section.name=value)
  
  [255]
  $ sl --cwd c --config a.b tip -q
  sl: parse errors: malformed --config option: 'a.b' (use --config section.name=value)
  
  [255]
  $ sl --cwd c --config a tip -q
  sl: parse errors: malformed --config option: 'a' (use --config section.name=value)
  
  [255]
  $ sl --cwd c --config a.= tip -q
  abort: malformed --config option: 'a.=' (use --config section.name=value)
  [255]
  $ sl --cwd c --config .b= tip -q
  abort: malformed --config option: '.b=' (use --config section.name=value)
  [255]

Testing --debug:

  $ sl --cwd c log --debug
  commit:      b6c483daf2907ce5825c0bb50f5716226281cc1a
  bookmark:    remote/master_b
  hoistedname: master_b
  phase:       public
  manifest:    23226e7a252cacdc2d99e4fbdc3653441056de49
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files+:      b
  extra:       branch=default
  description:
  b
  
  
  commit:      8580ff50825a50c8f716709acdf8de0deddcd6ab
  bookmark:    remote/master_a
  hoistedname: master_a
  phase:       public
  manifest:    a0c8bcbbb45c63b90b70ad007bf38961f64f2af0
  user:        test
  date:        Thu Jan 01 00:00:01 1970 +0000
  files+:      a
  extra:       branch=default
  description:
  a
  
  

Testing --traceback (this does not work with the Rust code path):

  $ sl --traceback log -r foo
  Traceback (most recent call last):
    # collapsed by devel.collapse-traceback
  sapling.error.RepoError: '$TESTTMP' is not inside a repository, but this command requires a repository
  abort: '$TESTTMP' is not inside a repository, but this command requires a repository!
  (use 'cd' to go to a directory inside a repository and try again)
  [255]

Testing --time:

  $ sl --cwd a --time id
  8580ff50825a
  time: real * (glob)

Testing --version:

  $ sl --version -q
  Sapling * (glob)

hide outer repo
  $ sl init

Testing -h/--help:

  $ sl -h
  Sapling SCM
  
  sl COMMAND [OPTIONS]
  
  These are some common Sapling commands.  Use 'sl help commands' to list all
  commands, and 'sl help COMMAND' to get help on a specific command.
  
  Get the latest commits from the server:
  
   pull          pull commits from the specified source
  
  View commits:
  
   show          show commit in detail
   diff          show differences between commits
  
  Check out a commit:
  
   goto          update working copy to a given commit
  
  Work with your checkout:
  
   status        list files with pending changes
   add           start tracking the specified files
   remove        delete the specified tracked files
   forget        stop tracking the specified files
   revert        change the specified files to match a commit
   purge         delete untracked files
  
  Commit changes and modify commits:
  
   commit        save all pending changes or specified files in a new commit
   amend         meld pending changes into the current commit
   metaedit      edit commit message and other metadata
  
  Rearrange commits:
  
   graft         copy commits from a different location
   hide          hide commits and their descendants
   unhide        unhide commits and their ancestors
  
  Work with stacks of commits:
  
   previous      check out an ancestor commit
   next          check out a descendant commit
   split         split a commit into smaller commits
   fold          combine multiple commits into a single commit
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
   unamend       undo the last amend operation on the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files
   web           launch Sapling Web GUI on localhost or a bound address
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates



  $ sl --help
  Sapling SCM
  
  sl COMMAND [OPTIONS]
  
  These are some common Sapling commands.  Use 'sl help commands' to list all
  commands, and 'sl help COMMAND' to get help on a specific command.
  
  Get the latest commits from the server:
  
   pull          pull commits from the specified source
  
  View commits:
  
   show          show commit in detail
   diff          show differences between commits
  
  Check out a commit:
  
   goto          update working copy to a given commit
  
  Work with your checkout:
  
   status        list files with pending changes
   add           start tracking the specified files
   remove        delete the specified tracked files
   forget        stop tracking the specified files
   revert        change the specified files to match a commit
   purge         delete untracked files
  
  Commit changes and modify commits:
  
   commit        save all pending changes or specified files in a new commit
   amend         meld pending changes into the current commit
   metaedit      edit commit message and other metadata
  
  Rearrange commits:
  
   graft         copy commits from a different location
   hide          hide commits and their descendants
   unhide        unhide commits and their ancestors
  
  Work with stacks of commits:
  
   previous      check out an ancestor commit
   next          check out a descendant commit
   split         split a commit into smaller commits
   fold          combine multiple commits into a single commit
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
   unamend       undo the last amend operation on the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files
   web           launch Sapling Web GUI on localhost or a bound address
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

Not tested: --debugger
