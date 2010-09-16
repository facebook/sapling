  $ hg init

  $ echo a > a
  $ hg ci -Ama
  adding a

  $ hg an a
  0: a

  $ echo "[ui]" >> $HGRCPATH
  $ echo "strict=True" >> $HGRCPATH

  $ hg an a
  hg: unknown command 'an'
  Mercurial Distributed SCM
  
  basic commands:
  
   add        add the specified files on the next commit
   annotate   show changeset information by line for each file
   clone      make a copy of an existing repository
   commit     commit the specified files or all outstanding changes
   diff       diff repository (or selected files)
   export     dump the header and diffs for one or more changesets
   forget     forget the specified files on the next commit
   init       create a new repository in the given directory
   log        show revision history of entire repository or files
   merge      merge working directory with another revision
   pull       pull changes from the specified source
   push       push changes to the specified destination
   remove     remove the specified files on the next commit
   serve      start stand-alone webserver
   status     show changed files in the working directory
   summary    summarize working directory state
   update     update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]
  $ hg annotate a
  0: a

should succeed - up is an alias, not an abbreviation

  $ hg up
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
