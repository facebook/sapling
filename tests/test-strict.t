  $ hg init

  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg an a
  0: a

  $ hg --config ui.strict=False an a
  0: a

  $ echo "[ui]" >> $HGRCPATH
  $ echo "strict=True" >> $HGRCPATH

  $ hg an a
  hg: unknown command 'an'
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
  
   checkout      checkout a specific commit
  
  Work with your checkout:
  
   status        show changed files in the working directory
   add           add the specified files on the next commit
   remove        remove the specified files on the next commit
   revert        restore files to their checkout state
   forget        forget the specified files on the next commit
  
  Commit changes and modify commits:
  
   commit        commit the specified files or all outstanding changes
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show combined config settings from all hgrc files
   grep          search for a pattern in tracked files in the working directory
  
  Additional help topics:
  
   filesets      specifying file sets
   glossary      glossary
   patterns      file name patterns
   revisions     specifying revisions
   templating    template usage
  [255]
  $ hg annotate a
  0: a

should succeed - up is an alias, not an abbreviation

  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
