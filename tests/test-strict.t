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
  
   diff          show differences between commits
   log           show commit history
   show          show commit in detail
  
  Checkout other commits:
  
   update        checkout a specific commit
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Exchange commits with a server:
  
   pull          pull changes from the specified source
   push          push changes to the specified destination
  
  Additional help topics:
  
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
