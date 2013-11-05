Short help:

  $ hg
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details

  $ hg -q
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)

  $ hg help
  Mercurial Distributed SCM
  
  list of commands:
  
   add           add the specified files on the next commit
   addremove     add all new files, delete all missing files
   annotate      show changeset information by line for each file
   archive       create an unversioned archive of a repository revision
   backout       reverse effect of earlier changeset
   bisect        subdivision search of changesets
   bookmarks     track a line of development with movable markers
   branch        set or show the current branch name
   branches      list repository named branches
   bundle        create a changegroup file
   cat           output the current or given revision of files
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   copy          mark files as copied for the next commit
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   graft         copy changes from other branches onto the current branch
   grep          search for a pattern in specified files and revisions
   heads         show branch heads
   help          show help for a given topic or a help overview
   identify      identify the working copy or specified revision
   import        import an ordered set of patches
   incoming      show new changesets found in source
   init          create a new repository in the given directory
   locate        locate files matching specific patterns
   log           show revision history of entire repository or files
   manifest      output the current or given revision of the project manifest
   merge         merge working directory with another revision
   outgoing      show changesets not found in the destination
   parents       show the parents of the working directory or revision
   paths         show aliases for remote repositories
   phase         set or show the current phase name
   pull          pull changes from the specified source
   push          push changes to the specified destination
   recover       roll back an interrupted transaction
   remove        remove the specified files on the next commit
   rename        rename files; equivalent of copy + remove
   resolve       redo merges or set/view the merge status of files
   revert        restore files to their checkout state
   root          print the root (top) of the current working directory
   serve         start stand-alone webserver
   showconfig    show combined config settings from all hgrc files
   status        show changed files in the working directory
   summary       summarize working directory state
   tag           add one or more tags for the current or given revision
   tags          list repository tags
   unbundle      apply one or more changegroup files
   update        update working directory (or switch revisions)
   verify        verify the integrity of the repository
   version       output version and copyright information
  
  additional help topics:
  
   config        Configuration Files
   dates         Date Formats
   diffs         Diff Formats
   environment   Environment Variables
   extensions    Using Additional Features
   filesets      Specifying File Sets
   glossary      Glossary
   hgignore      Syntax for Mercurial Ignore Files
   hgweb         Configuring hgweb
   merge-tools   Merge Tools
   multirevs     Specifying Multiple Revisions
   patterns      File Name Patterns
   phases        Working with Phases
   revisions     Specifying Single Revisions
   revsets       Specifying Revision Sets
   subrepos      Subrepositories
   templating    Template Usage
   urls          URL Paths
  
  use "hg -v help" to show builtin aliases and global options

  $ hg -q help
   add           add the specified files on the next commit
   addremove     add all new files, delete all missing files
   annotate      show changeset information by line for each file
   archive       create an unversioned archive of a repository revision
   backout       reverse effect of earlier changeset
   bisect        subdivision search of changesets
   bookmarks     track a line of development with movable markers
   branch        set or show the current branch name
   branches      list repository named branches
   bundle        create a changegroup file
   cat           output the current or given revision of files
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   copy          mark files as copied for the next commit
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   graft         copy changes from other branches onto the current branch
   grep          search for a pattern in specified files and revisions
   heads         show branch heads
   help          show help for a given topic or a help overview
   identify      identify the working copy or specified revision
   import        import an ordered set of patches
   incoming      show new changesets found in source
   init          create a new repository in the given directory
   locate        locate files matching specific patterns
   log           show revision history of entire repository or files
   manifest      output the current or given revision of the project manifest
   merge         merge working directory with another revision
   outgoing      show changesets not found in the destination
   parents       show the parents of the working directory or revision
   paths         show aliases for remote repositories
   phase         set or show the current phase name
   pull          pull changes from the specified source
   push          push changes to the specified destination
   recover       roll back an interrupted transaction
   remove        remove the specified files on the next commit
   rename        rename files; equivalent of copy + remove
   resolve       redo merges or set/view the merge status of files
   revert        restore files to their checkout state
   root          print the root (top) of the current working directory
   serve         start stand-alone webserver
   showconfig    show combined config settings from all hgrc files
   status        show changed files in the working directory
   summary       summarize working directory state
   tag           add one or more tags for the current or given revision
   tags          list repository tags
   unbundle      apply one or more changegroup files
   update        update working directory (or switch revisions)
   verify        verify the integrity of the repository
   version       output version and copyright information
  
  additional help topics:
  
   config        Configuration Files
   dates         Date Formats
   diffs         Diff Formats
   environment   Environment Variables
   extensions    Using Additional Features
   filesets      Specifying File Sets
   glossary      Glossary
   hgignore      Syntax for Mercurial Ignore Files
   hgweb         Configuring hgweb
   merge-tools   Merge Tools
   multirevs     Specifying Multiple Revisions
   patterns      File Name Patterns
   phases        Working with Phases
   revisions     Specifying Single Revisions
   revsets       Specifying Revision Sets
   subrepos      Subrepositories
   templating    Template Usage
   urls          URL Paths

Test short command list with verbose option

  $ hg -v help shortlist
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate, blame
                 show changeset information by line for each file
   clone         make a copy of an existing repository
   commit, ci    commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log, history  show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove, rm    remove the specified files on the next commit
   serve         start stand-alone webserver
   status, st    show changed files in the working directory
   summary, sum  summarize working directory state
   update, up, checkout, co
                 update working directory (or switch revisions)
  
  global options:
  
   -R --repository REPO   repository root directory or name of overlay bundle
                          file
      --cwd DIR           change working directory
   -y --noninteractive    do not prompt, automatically pick the first choice for
                          all prompts
   -q --quiet             suppress output
   -v --verbose           enable additional output
      --config CONFIG [+] set/override config option (use 'section.name=value')
      --debug             enable debugging output
      --debugger          start debugger
      --encoding ENCODE   set the charset encoding (default: ascii)
      --encodingmode MODE set the charset encoding mode (default: strict)
      --traceback         always print a traceback on exception
      --time              time how long the command takes
      --profile           print command execution profile
      --version           output version information and exit
   -h --help              display help and exit
      --hidden            consider hidden changesets
  
  [+] marked option can be specified multiple times
  
  use "hg help" for the full list of commands

  $ hg add -h
  hg add [OPTION]... [FILE]...
  
  add the specified files on the next commit
  
      Schedule files to be version controlled and added to the repository.
  
      The files will be added to the repository at the next commit. To undo an
      add before that, see "hg forget".
  
      If no names are given, add all files to the repository.
  
      Returns 0 if all files are successfully added.
  
  options:
  
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
   -n --dry-run             do not perform actions, just print output
  
  [+] marked option can be specified multiple times
  
  use "hg -v help add" to show more complete help and the global options

Verbose help for add

  $ hg add -hv
  hg add [OPTION]... [FILE]...
  
  add the specified files on the next commit
  
      Schedule files to be version controlled and added to the repository.
  
      The files will be added to the repository at the next commit. To undo an
      add before that, see "hg forget".
  
      If no names are given, add all files to the repository.
  
      An example showing how new (unknown) files are added automatically by "hg
      add":
  
        $ ls
        foo.c
        $ hg status
        ? foo.c
        $ hg add
        adding foo.c
        $ hg status
        A foo.c
  
      Returns 0 if all files are successfully added.
  
  options:
  
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
   -n --dry-run             do not perform actions, just print output
  
  [+] marked option can be specified multiple times
  
  global options:
  
   -R --repository REPO   repository root directory or name of overlay bundle
                          file
      --cwd DIR           change working directory
   -y --noninteractive    do not prompt, automatically pick the first choice for
                          all prompts
   -q --quiet             suppress output
   -v --verbose           enable additional output
      --config CONFIG [+] set/override config option (use 'section.name=value')
      --debug             enable debugging output
      --debugger          start debugger
      --encoding ENCODE   set the charset encoding (default: ascii)
      --encodingmode MODE set the charset encoding mode (default: strict)
      --traceback         always print a traceback on exception
      --time              time how long the command takes
      --profile           print command execution profile
      --version           output version information and exit
   -h --help              display help and exit
      --hidden            consider hidden changesets
  
  [+] marked option can be specified multiple times

Test help option with version option

  $ hg add -h --version
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-2013 Matt Mackall and others
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

  $ hg add --skjdfks
  hg add: option --skjdfks not recognized
  hg add [OPTION]... [FILE]...
  
  add the specified files on the next commit
  
  options:
  
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
   -n --dry-run             do not perform actions, just print output
  
  [+] marked option can be specified multiple times
  
  use "hg help add" to show the full help text
  [255]

Test ambiguous command help

  $ hg help ad
  list of commands:
  
   add           add the specified files on the next commit
   addremove     add all new files, delete all missing files
  
  use "hg -v help ad" to show builtin aliases and global options

Test command without options

  $ hg help verify
  hg verify
  
  verify the integrity of the repository
  
      Verify the integrity of the current repository.
  
      This will perform an extensive check of the repository's integrity,
      validating the hashes and checksums of each entry in the changelog,
      manifest, and tracked files, as well as the integrity of their crosslinks
      and indices.
  
      Please see http://mercurial.selenic.com/wiki/RepositoryCorruption for more
      information about recovery from corruption of the repository.
  
      Returns 0 on success, 1 if errors are encountered.
  
  use "hg -v help verify" to show the global options

  $ hg help diff
  hg diff [OPTION]... ([-c REV] | [-r REV1 [-r REV2]]) [FILE]...
  
  diff repository (or selected files)
  
      Show differences between revisions for the specified files.
  
      Differences between files are shown using the unified diff format.
  
      Note:
         diff may generate unexpected results for merges, as it will default to
         comparing against the working directory's first parent changeset if no
         revisions are specified.
  
      When two revision arguments are given, then changes are shown between
      those revisions. If only one revision is specified then that revision is
      compared to the working directory, and, when no revisions are specified,
      the working directory files are compared to its parent.
  
      Alternatively you can specify -c/--change with a revision to see the
      changes in that changeset relative to its first parent.
  
      Without the -a/--text option, diff will avoid generating diffs of files it
      detects as binary. With -a, diff will generate a diff anyway, probably
      with undesirable results.
  
      Use the -g/--git option to generate diffs in the git extended diff format.
      For more information, read "hg help diffs".
  
      Returns 0 on success.
  
  options:
  
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -a --text                treat all files as text
   -g --git                 use git extended diff format
      --nodates             omit dates from diff headers
   -p --show-function       show which function each change is in
      --reverse             produce a diff that undoes the changes
   -w --ignore-all-space    ignore white space when comparing lines
   -b --ignore-space-change ignore changes in the amount of white space
   -B --ignore-blank-lines  ignore changes whose lines are all blank
   -U --unified NUM         number of lines of context to show
      --stat                output diffstat-style summary of changes
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
  
  [+] marked option can be specified multiple times
  
  use "hg -v help diff" to show more complete help and the global options

  $ hg help status
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  show changed files in the working directory
  
      Show status of files in the repository. If names are given, only files
      that match are shown. Files that are clean or ignored or the source of a
      copy/move operation, are not listed unless -c/--clean, -i/--ignored,
      -C/--copies or -A/--all are given. Unless options described with "show
      only ..." are given, the options -mardu are used.
  
      Option -q/--quiet hides untracked (unknown and ignored) files unless
      explicitly requested with -u/--unknown or -i/--ignored.
  
      Note:
         status may appear to disagree with diff if permissions have changed or
         a merge has occurred. The standard diff format does not report
         permission changes and diff only reports changes relative to one merge
         parent.
  
      If one revision is given, it is used as the base revision. If two
      revisions are given, the differences between them are shown. The --change
      option can also be used as a shortcut to list the changed files of a
      revision from its first parent.
  
      The codes used to show the status of files are:
  
        M = modified
        A = added
        R = removed
        C = clean
        ! = missing (deleted by non-hg command, but still tracked)
        ? = not tracked
        I = ignored
          = origin of the previous file listed as A (added)
  
      Returns 0 on success.
  
  options:
  
   -A --all                 show status of all files
   -m --modified            show only modified files
   -a --added               show only added files
   -r --removed             show only removed files
   -d --deleted             show only deleted (but tracked) files
   -c --clean               show only files without changes
   -u --unknown             show only unknown (not tracked) files
   -i --ignored             show only ignored files
   -n --no-status           hide status prefix
   -C --copies              show source of copied files
   -0 --print0              end filenames with NUL, for use with xargs
      --rev REV [+]         show difference from revision
      --change REV          list the changed files of a revision
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
  
  [+] marked option can be specified multiple times
  
  use "hg -v help status" to show more complete help and the global options

  $ hg -q help status
  hg status [OPTION]... [FILE]...
  
  show changed files in the working directory

  $ hg help foo
  hg: unknown command 'foo'
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]

  $ hg skjdfks
  hg: unknown command 'skjdfks'
  Mercurial Distributed SCM
  
  basic commands:
  
   add           add the specified files on the next commit
   annotate      show changeset information by line for each file
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   init          create a new repository in the given directory
   log           show revision history of entire repository or files
   merge         merge working directory with another revision
   pull          pull changes from the specified source
   push          push changes to the specified destination
   remove        remove the specified files on the next commit
   serve         start stand-alone webserver
   status        show changed files in the working directory
   summary       summarize working directory state
   update        update working directory (or switch revisions)
  
  use "hg help" for the full list of commands or "hg -v" for details
  [255]

  $ cat > helpext.py <<EOF
  > import os
  > from mercurial import commands
  > 
  > def nohelp(ui, *args, **kwargs):
  >     pass
  > 
  > cmdtable = {
  >     "nohelp": (nohelp, [], "hg nohelp"),
  > }
  > 
  > commands.norepo += ' nohelp'
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "helpext = `pwd`/helpext.py" >> $HGRCPATH

Test command with no help text

  $ hg help nohelp
  hg nohelp
  
  (no help text available)
  
  use "hg -v help nohelp" to show the global options

  $ hg help -k nohelp
  Commands:
  
   nohelp hg nohelp
  
  Extension Commands:
  
   nohelp (no help text available)

Test that default list of commands omits extension commands

  $ hg help
  Mercurial Distributed SCM
  
  list of commands:
  
   add           add the specified files on the next commit
   addremove     add all new files, delete all missing files
   annotate      show changeset information by line for each file
   archive       create an unversioned archive of a repository revision
   backout       reverse effect of earlier changeset
   bisect        subdivision search of changesets
   bookmarks     track a line of development with movable markers
   branch        set or show the current branch name
   branches      list repository named branches
   bundle        create a changegroup file
   cat           output the current or given revision of files
   clone         make a copy of an existing repository
   commit        commit the specified files or all outstanding changes
   copy          mark files as copied for the next commit
   diff          diff repository (or selected files)
   export        dump the header and diffs for one or more changesets
   forget        forget the specified files on the next commit
   graft         copy changes from other branches onto the current branch
   grep          search for a pattern in specified files and revisions
   heads         show branch heads
   help          show help for a given topic or a help overview
   identify      identify the working copy or specified revision
   import        import an ordered set of patches
   incoming      show new changesets found in source
   init          create a new repository in the given directory
   locate        locate files matching specific patterns
   log           show revision history of entire repository or files
   manifest      output the current or given revision of the project manifest
   merge         merge working directory with another revision
   outgoing      show changesets not found in the destination
   parents       show the parents of the working directory or revision
   paths         show aliases for remote repositories
   phase         set or show the current phase name
   pull          pull changes from the specified source
   push          push changes to the specified destination
   recover       roll back an interrupted transaction
   remove        remove the specified files on the next commit
   rename        rename files; equivalent of copy + remove
   resolve       redo merges or set/view the merge status of files
   revert        restore files to their checkout state
   root          print the root (top) of the current working directory
   serve         start stand-alone webserver
   showconfig    show combined config settings from all hgrc files
   status        show changed files in the working directory
   summary       summarize working directory state
   tag           add one or more tags for the current or given revision
   tags          list repository tags
   unbundle      apply one or more changegroup files
   update        update working directory (or switch revisions)
   verify        verify the integrity of the repository
   version       output version and copyright information
  
  enabled extensions:
  
   helpext       (no help text available)
  
  additional help topics:
  
   config        Configuration Files
   dates         Date Formats
   diffs         Diff Formats
   environment   Environment Variables
   extensions    Using Additional Features
   filesets      Specifying File Sets
   glossary      Glossary
   hgignore      Syntax for Mercurial Ignore Files
   hgweb         Configuring hgweb
   merge-tools   Merge Tools
   multirevs     Specifying Multiple Revisions
   patterns      File Name Patterns
   phases        Working with Phases
   revisions     Specifying Single Revisions
   revsets       Specifying Revision Sets
   subrepos      Subrepositories
   templating    Template Usage
   urls          URL Paths
  
  use "hg -v help" to show builtin aliases and global options



Test list of commands with command with no help text

  $ hg help helpext
  helpext extension - no help text available
  
  list of commands:
  
   nohelp        (no help text available)
  
  use "hg -v help helpext" to show builtin aliases and global options

Test a help topic

  $ hg help revs
  Specifying Single Revisions
  """""""""""""""""""""""""""
  
      Mercurial supports several ways to specify individual revisions.
  
      A plain integer is treated as a revision number. Negative integers are
      treated as sequential offsets from the tip, with -1 denoting the tip, -2
      denoting the revision prior to the tip, and so forth.
  
      A 40-digit hexadecimal string is treated as a unique revision identifier.
  
      A hexadecimal string less than 40 characters long is treated as a unique
      revision identifier and is referred to as a short-form identifier. A
      short-form identifier is only valid if it is the prefix of exactly one
      full-length identifier.
  
      Any other string is treated as a bookmark, tag, or branch name. A bookmark
      is a movable pointer to a revision. A tag is a permanent name associated
      with a revision. A branch name denotes the tipmost revision of that
      branch. Bookmark, tag, and branch names must not contain the ":"
      character.
  
      The reserved name "tip" always identifies the most recent revision.
  
      The reserved name "null" indicates the null revision. This is the revision
      of an empty repository, and the parent of revision 0.
  
      The reserved name "." indicates the working directory parent. If no
      working directory is checked out, it is equivalent to null. If an
      uncommitted merge is in progress, "." is the revision of the first parent.

Test templating help

  $ hg help templating | egrep '(desc|diffstat|firstline|nonempty)  '
      desc          String. The text of the changeset description.
      diffstat      String. Statistics of changes with the following format:
      firstline     Any text. Returns the first line of text.
      nonempty      Any text. Returns '(none)' if the string is empty.

Test help hooks

  $ cat > helphook1.py <<EOF
  > from mercurial import help
  > 
  > def rewrite(topic, doc):
  >     return doc + '\nhelphook1\n'
  > 
  > def extsetup(ui):
  >     help.addtopichook('revsets', rewrite)
  > EOF
  $ cat > helphook2.py <<EOF
  > from mercurial import help
  > 
  > def rewrite(topic, doc):
  >     return doc + '\nhelphook2\n'
  > 
  > def extsetup(ui):
  >     help.addtopichook('revsets', rewrite)
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "helphook1 = `pwd`/helphook1.py" >> $HGRCPATH
  $ echo "helphook2 = `pwd`/helphook2.py" >> $HGRCPATH
  $ hg help revsets | grep helphook
      helphook1
      helphook2

Test keyword search help

  $ cat > prefixedname.py <<EOF
  > '''matched against word "clone"
  > '''
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "dot.dot.prefixedname = `pwd`/prefixedname.py" >> $HGRCPATH
  $ hg help -k clone
  Topics:
  
   config     Configuration Files
   extensions Using Additional Features
   glossary   Glossary
   phases     Working with Phases
   subrepos   Subrepositories
   urls       URL Paths
  
  Commands:
  
   bookmarks track a line of development with movable markers
   clone     make a copy of an existing repository
   paths     show aliases for remote repositories
   update    update working directory (or switch revisions)
  
  Extensions:
  
   prefixedname matched against word "clone"
   relink       recreates hardlinks between repository clones
  
  Extension Commands:
  
   qclone clone main and patch repository at same time

Test omit indicating for help

  $ cat > addverboseitems.py <<EOF
  > '''extension to test omit indicating.
  > 
  > This paragraph is never omitted (for extension)
  > 
  > .. container:: verbose
  > 
  >   This paragraph is omitted,
  >   if :hg:\`help\` is invoked witout \`\`-v\`\` (for extension)
  > 
  > This paragraph is never omitted, too (for extension)
  > '''
  > 
  > from mercurial import help, commands
  > testtopic = """This paragraph is never omitted (for topic).
  > 
  > .. container:: verbose
  > 
  >   This paragraph is omitted,
  >   if :hg:\`help\` is invoked witout \`\`-v\`\` (for topic)
  > 
  > This paragraph is never omitted, too (for topic)
  > """
  > def extsetup(ui):
  >     help.helptable.append((["topic-containing-verbose"],
  >                            "This is the topic to test omit indicating.",
  >                            lambda : testtopic))
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "addverboseitems = `pwd`/addverboseitems.py" >> $HGRCPATH
  $ hg help addverboseitems
  addverboseitems extension - extension to test omit indicating.
  
  This paragraph is never omitted (for extension)
  
  This paragraph is never omitted, too (for extension)
  
  use "hg help -v addverboseitems" to show more complete help
  
  no commands defined
  $ hg help -v addverboseitems
  addverboseitems extension - extension to test omit indicating.
  
  This paragraph is never omitted (for extension)
  
  This paragraph is omitted, if "hg help" is invoked witout "-v" (for extension)
  
  This paragraph is never omitted, too (for extension)
  
  no commands defined
  $ hg help topic-containing-verbose
  This is the topic to test omit indicating.
  """"""""""""""""""""""""""""""""""""""""""
  
      This paragraph is never omitted (for topic).
  
      This paragraph is never omitted, too (for topic)
  
  use "hg help -v topic-containing-verbose" to show more complete help
  $ hg help -v topic-containing-verbose
  This is the topic to test omit indicating.
  """"""""""""""""""""""""""""""""""""""""""
  
      This paragraph is never omitted (for topic).
  
      This paragraph is omitted, if "hg help" is invoked witout "-v" (for topic)
  
      This paragraph is never omitted, too (for topic)

Test usage of section marks in help documents

  $ cd "$TESTDIR"/../doc
  $ python check-seclevel.py
  $ cd $TESTTMP

#if serve

Test the help pages in hgweb.

Dish up an empty repo; serve it cold.

  $ hg init "$TESTTMP/test"
  $ hg serve -R "$TESTTMP/test" -n test -p $HGPORT -d --pid-file=hg.pid
  $ cat hg.pid >> $DAEMON_PIDS

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT "help"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>Help: Index</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
  <li class="active">help</li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  <table class="bigtable">
  <tr><td colspan="2"><h2><a name="main" href="#topics">Topics</a></h2></td></tr>
  
  <tr><td>
  <a href="/help/config">
  config
  </a>
  </td><td>
  Configuration Files
  </td></tr>
  <tr><td>
  <a href="/help/dates">
  dates
  </a>
  </td><td>
  Date Formats
  </td></tr>
  <tr><td>
  <a href="/help/diffs">
  diffs
  </a>
  </td><td>
  Diff Formats
  </td></tr>
  <tr><td>
  <a href="/help/environment">
  environment
  </a>
  </td><td>
  Environment Variables
  </td></tr>
  <tr><td>
  <a href="/help/extensions">
  extensions
  </a>
  </td><td>
  Using Additional Features
  </td></tr>
  <tr><td>
  <a href="/help/filesets">
  filesets
  </a>
  </td><td>
  Specifying File Sets
  </td></tr>
  <tr><td>
  <a href="/help/glossary">
  glossary
  </a>
  </td><td>
  Glossary
  </td></tr>
  <tr><td>
  <a href="/help/hgignore">
  hgignore
  </a>
  </td><td>
  Syntax for Mercurial Ignore Files
  </td></tr>
  <tr><td>
  <a href="/help/hgweb">
  hgweb
  </a>
  </td><td>
  Configuring hgweb
  </td></tr>
  <tr><td>
  <a href="/help/merge-tools">
  merge-tools
  </a>
  </td><td>
  Merge Tools
  </td></tr>
  <tr><td>
  <a href="/help/multirevs">
  multirevs
  </a>
  </td><td>
  Specifying Multiple Revisions
  </td></tr>
  <tr><td>
  <a href="/help/patterns">
  patterns
  </a>
  </td><td>
  File Name Patterns
  </td></tr>
  <tr><td>
  <a href="/help/phases">
  phases
  </a>
  </td><td>
  Working with Phases
  </td></tr>
  <tr><td>
  <a href="/help/revisions">
  revisions
  </a>
  </td><td>
  Specifying Single Revisions
  </td></tr>
  <tr><td>
  <a href="/help/revsets">
  revsets
  </a>
  </td><td>
  Specifying Revision Sets
  </td></tr>
  <tr><td>
  <a href="/help/subrepos">
  subrepos
  </a>
  </td><td>
  Subrepositories
  </td></tr>
  <tr><td>
  <a href="/help/templating">
  templating
  </a>
  </td><td>
  Template Usage
  </td></tr>
  <tr><td>
  <a href="/help/urls">
  urls
  </a>
  </td><td>
  URL Paths
  </td></tr>
  <tr><td>
  <a href="/help/topic-containing-verbose">
  topic-containing-verbose
  </a>
  </td><td>
  This is the topic to test omit indicating.
  </td></tr>
  
  <tr><td colspan="2"><h2><a name="main" href="#main">Main Commands</a></h2></td></tr>
  
  <tr><td>
  <a href="/help/add">
  add
  </a>
  </td><td>
  add the specified files on the next commit
  </td></tr>
  <tr><td>
  <a href="/help/annotate">
  annotate
  </a>
  </td><td>
  show changeset information by line for each file
  </td></tr>
  <tr><td>
  <a href="/help/clone">
  clone
  </a>
  </td><td>
  make a copy of an existing repository
  </td></tr>
  <tr><td>
  <a href="/help/commit">
  commit
  </a>
  </td><td>
  commit the specified files or all outstanding changes
  </td></tr>
  <tr><td>
  <a href="/help/diff">
  diff
  </a>
  </td><td>
  diff repository (or selected files)
  </td></tr>
  <tr><td>
  <a href="/help/export">
  export
  </a>
  </td><td>
  dump the header and diffs for one or more changesets
  </td></tr>
  <tr><td>
  <a href="/help/forget">
  forget
  </a>
  </td><td>
  forget the specified files on the next commit
  </td></tr>
  <tr><td>
  <a href="/help/init">
  init
  </a>
  </td><td>
  create a new repository in the given directory
  </td></tr>
  <tr><td>
  <a href="/help/log">
  log
  </a>
  </td><td>
  show revision history of entire repository or files
  </td></tr>
  <tr><td>
  <a href="/help/merge">
  merge
  </a>
  </td><td>
  merge working directory with another revision
  </td></tr>
  <tr><td>
  <a href="/help/pull">
  pull
  </a>
  </td><td>
  pull changes from the specified source
  </td></tr>
  <tr><td>
  <a href="/help/push">
  push
  </a>
  </td><td>
  push changes to the specified destination
  </td></tr>
  <tr><td>
  <a href="/help/remove">
  remove
  </a>
  </td><td>
  remove the specified files on the next commit
  </td></tr>
  <tr><td>
  <a href="/help/serve">
  serve
  </a>
  </td><td>
  start stand-alone webserver
  </td></tr>
  <tr><td>
  <a href="/help/status">
  status
  </a>
  </td><td>
  show changed files in the working directory
  </td></tr>
  <tr><td>
  <a href="/help/summary">
  summary
  </a>
  </td><td>
  summarize working directory state
  </td></tr>
  <tr><td>
  <a href="/help/update">
  update
  </a>
  </td><td>
  update working directory (or switch revisions)
  </td></tr>
  
  <tr><td colspan="2"><h2><a name="other" href="#other">Other Commands</a></h2></td></tr>
  
  <tr><td>
  <a href="/help/addremove">
  addremove
  </a>
  </td><td>
  add all new files, delete all missing files
  </td></tr>
  <tr><td>
  <a href="/help/archive">
  archive
  </a>
  </td><td>
  create an unversioned archive of a repository revision
  </td></tr>
  <tr><td>
  <a href="/help/backout">
  backout
  </a>
  </td><td>
  reverse effect of earlier changeset
  </td></tr>
  <tr><td>
  <a href="/help/bisect">
  bisect
  </a>
  </td><td>
  subdivision search of changesets
  </td></tr>
  <tr><td>
  <a href="/help/bookmarks">
  bookmarks
  </a>
  </td><td>
  track a line of development with movable markers
  </td></tr>
  <tr><td>
  <a href="/help/branch">
  branch
  </a>
  </td><td>
  set or show the current branch name
  </td></tr>
  <tr><td>
  <a href="/help/branches">
  branches
  </a>
  </td><td>
  list repository named branches
  </td></tr>
  <tr><td>
  <a href="/help/bundle">
  bundle
  </a>
  </td><td>
  create a changegroup file
  </td></tr>
  <tr><td>
  <a href="/help/cat">
  cat
  </a>
  </td><td>
  output the current or given revision of files
  </td></tr>
  <tr><td>
  <a href="/help/copy">
  copy
  </a>
  </td><td>
  mark files as copied for the next commit
  </td></tr>
  <tr><td>
  <a href="/help/graft">
  graft
  </a>
  </td><td>
  copy changes from other branches onto the current branch
  </td></tr>
  <tr><td>
  <a href="/help/grep">
  grep
  </a>
  </td><td>
  search for a pattern in specified files and revisions
  </td></tr>
  <tr><td>
  <a href="/help/heads">
  heads
  </a>
  </td><td>
  show branch heads
  </td></tr>
  <tr><td>
  <a href="/help/help">
  help
  </a>
  </td><td>
  show help for a given topic or a help overview
  </td></tr>
  <tr><td>
  <a href="/help/identify">
  identify
  </a>
  </td><td>
  identify the working copy or specified revision
  </td></tr>
  <tr><td>
  <a href="/help/import">
  import
  </a>
  </td><td>
  import an ordered set of patches
  </td></tr>
  <tr><td>
  <a href="/help/incoming">
  incoming
  </a>
  </td><td>
  show new changesets found in source
  </td></tr>
  <tr><td>
  <a href="/help/locate">
  locate
  </a>
  </td><td>
  locate files matching specific patterns
  </td></tr>
  <tr><td>
  <a href="/help/manifest">
  manifest
  </a>
  </td><td>
  output the current or given revision of the project manifest
  </td></tr>
  <tr><td>
  <a href="/help/nohelp">
  nohelp
  </a>
  </td><td>
  (no help text available)
  </td></tr>
  <tr><td>
  <a href="/help/outgoing">
  outgoing
  </a>
  </td><td>
  show changesets not found in the destination
  </td></tr>
  <tr><td>
  <a href="/help/parents">
  parents
  </a>
  </td><td>
  show the parents of the working directory or revision
  </td></tr>
  <tr><td>
  <a href="/help/paths">
  paths
  </a>
  </td><td>
  show aliases for remote repositories
  </td></tr>
  <tr><td>
  <a href="/help/phase">
  phase
  </a>
  </td><td>
  set or show the current phase name
  </td></tr>
  <tr><td>
  <a href="/help/recover">
  recover
  </a>
  </td><td>
  roll back an interrupted transaction
  </td></tr>
  <tr><td>
  <a href="/help/rename">
  rename
  </a>
  </td><td>
  rename files; equivalent of copy + remove
  </td></tr>
  <tr><td>
  <a href="/help/resolve">
  resolve
  </a>
  </td><td>
  redo merges or set/view the merge status of files
  </td></tr>
  <tr><td>
  <a href="/help/revert">
  revert
  </a>
  </td><td>
  restore files to their checkout state
  </td></tr>
  <tr><td>
  <a href="/help/root">
  root
  </a>
  </td><td>
  print the root (top) of the current working directory
  </td></tr>
  <tr><td>
  <a href="/help/showconfig">
  showconfig
  </a>
  </td><td>
  show combined config settings from all hgrc files
  </td></tr>
  <tr><td>
  <a href="/help/tag">
  tag
  </a>
  </td><td>
  add one or more tags for the current or given revision
  </td></tr>
  <tr><td>
  <a href="/help/tags">
  tags
  </a>
  </td><td>
  list repository tags
  </td></tr>
  <tr><td>
  <a href="/help/unbundle">
  unbundle
  </a>
  </td><td>
  apply one or more changegroup files
  </td></tr>
  <tr><td>
  <a href="/help/verify">
  verify
  </a>
  </td><td>
  verify the integrity of the repository
  </td></tr>
  <tr><td>
  <a href="/help/version">
  version
  </a>
  </td><td>
  output version and copyright information
  </td></tr>
  </table>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT "help/add"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>Help: add</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active"><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>Help: add</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  <div id="doc">
  <p>
  hg add [OPTION]... [FILE]...
  </p>
  <p>
  add the specified files on the next commit
  </p>
  <p>
  Schedule files to be version controlled and added to the
  repository.
  </p>
  <p>
  The files will be added to the repository at the next commit. To
  undo an add before that, see &quot;hg forget&quot;.
  </p>
  <p>
  If no names are given, add all files to the repository.
  </p>
  <p>
  An example showing how new (unknown) files are added
  automatically by &quot;hg add&quot;:
  </p>
  <pre>
  \$ ls (re)
  foo.c
  \$ hg status (re)
  ? foo.c
  \$ hg add (re)
  adding foo.c
  \$ hg status (re)
  A foo.c
  </pre>
  <p>
  Returns 0 if all files are successfully added.
  </p>
  <p>
  options:
  </p>
  <table>
  <tr><td>-I</td>
  <td>--include PATTERN [+]</td>
  <td>include names matching the given patterns</td></tr>
  <tr><td>-X</td>
  <td>--exclude PATTERN [+]</td>
  <td>exclude names matching the given patterns</td></tr>
  <tr><td>-S</td>
  <td>--subrepos</td>
  <td>recurse into subrepositories</td></tr>
  <tr><td>-n</td>
  <td>--dry-run</td>
  <td>do not perform actions, just print output</td></tr>
  </table>
  <p>
  [+] marked option can be specified multiple times
  </p>
  <p>
  global options:
  </p>
  <table>
  <tr><td>-R</td>
  <td>--repository REPO</td>
  <td>repository root directory or name of overlay bundle file</td></tr>
  <tr><td></td>
  <td>--cwd DIR</td>
  <td>change working directory</td></tr>
  <tr><td>-y</td>
  <td>--noninteractive</td>
  <td>do not prompt, automatically pick the first choice for all prompts</td></tr>
  <tr><td>-q</td>
  <td>--quiet</td>
  <td>suppress output</td></tr>
  <tr><td>-v</td>
  <td>--verbose</td>
  <td>enable additional output</td></tr>
  <tr><td></td>
  <td>--config CONFIG [+]</td>
  <td>set/override config option (use 'section.name=value')</td></tr>
  <tr><td></td>
  <td>--debug</td>
  <td>enable debugging output</td></tr>
  <tr><td></td>
  <td>--debugger</td>
  <td>start debugger</td></tr>
  <tr><td></td>
  <td>--encoding ENCODE</td>
  <td>set the charset encoding (default: ascii)</td></tr>
  <tr><td></td>
  <td>--encodingmode MODE</td>
  <td>set the charset encoding mode (default: strict)</td></tr>
  <tr><td></td>
  <td>--traceback</td>
  <td>always print a traceback on exception</td></tr>
  <tr><td></td>
  <td>--time</td>
  <td>time how long the command takes</td></tr>
  <tr><td></td>
  <td>--profile</td>
  <td>print command execution profile</td></tr>
  <tr><td></td>
  <td>--version</td>
  <td>output version information and exit</td></tr>
  <tr><td>-h</td>
  <td>--help</td>
  <td>display help and exit</td></tr>
  <tr><td></td>
  <td>--hidden</td>
  <td>consider hidden changesets</td></tr>
  </table>
  <p>
  [+] marked option can be specified multiple times
  </p>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT "help/remove"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>Help: remove</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active"><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>Help: remove</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  <div id="doc">
  <p>
  hg remove [OPTION]... FILE...
  </p>
  <p>
  aliases: rm
  </p>
  <p>
  remove the specified files on the next commit
  </p>
  <p>
  Schedule the indicated files for removal from the current branch.
  </p>
  <p>
  This command schedules the files to be removed at the next commit.
  To undo a remove before that, see &quot;hg revert&quot;. To undo added
  files, see &quot;hg forget&quot;.
  </p>
  <p>
  -A/--after can be used to remove only files that have already
  been deleted, -f/--force can be used to force deletion, and -Af
  can be used to remove files from the next revision without
  deleting them from the working directory.
  </p>
  <p>
  The following table details the behavior of remove for different
  file states (columns) and option combinations (rows). The file
  states are Added [A], Clean [C], Modified [M] and Missing [!]
  (as reported by &quot;hg status&quot;). The actions are Warn, Remove
  (from branch) and Delete (from disk):
  </p>
  <table>
  <tr><td>opt/state</td>
  <td>A</td>
  <td>C</td>
  <td>M</td>
  <td>!</td></tr>
  <tr><td>none</td>
  <td>W</td>
  <td>RD</td>
  <td>W</td>
  <td>R</td></tr>
  <tr><td>-f</td>
  <td>R</td>
  <td>RD</td>
  <td>RD</td>
  <td>R</td></tr>
  <tr><td>-A</td>
  <td>W</td>
  <td>W</td>
  <td>W</td>
  <td>R</td></tr>
  <tr><td>-Af</td>
  <td>R</td>
  <td>R</td>
  <td>R</td>
  <td>R</td></tr>
  </table>
  <p>
  Note that remove never deletes files in Added [A] state from the
  working directory, not even if option --force is specified.
  </p>
  <p>
  Returns 0 on success, 1 if any warnings encountered.
  </p>
  <p>
  options:
  </p>
  <table>
  <tr><td>-A</td>
  <td>--after</td>
  <td>record delete for missing files</td></tr>
  <tr><td>-f</td>
  <td>--force</td>
  <td>remove (and delete) file even if added or modified</td></tr>
  <tr><td>-I</td>
  <td>--include PATTERN [+]</td>
  <td>include names matching the given patterns</td></tr>
  <tr><td>-X</td>
  <td>--exclude PATTERN [+]</td>
  <td>exclude names matching the given patterns</td></tr>
  </table>
  <p>
  [+] marked option can be specified multiple times
  </p>
  <p>
  global options:
  </p>
  <table>
  <tr><td>-R</td>
  <td>--repository REPO</td>
  <td>repository root directory or name of overlay bundle file</td></tr>
  <tr><td></td>
  <td>--cwd DIR</td>
  <td>change working directory</td></tr>
  <tr><td>-y</td>
  <td>--noninteractive</td>
  <td>do not prompt, automatically pick the first choice for all prompts</td></tr>
  <tr><td>-q</td>
  <td>--quiet</td>
  <td>suppress output</td></tr>
  <tr><td>-v</td>
  <td>--verbose</td>
  <td>enable additional output</td></tr>
  <tr><td></td>
  <td>--config CONFIG [+]</td>
  <td>set/override config option (use 'section.name=value')</td></tr>
  <tr><td></td>
  <td>--debug</td>
  <td>enable debugging output</td></tr>
  <tr><td></td>
  <td>--debugger</td>
  <td>start debugger</td></tr>
  <tr><td></td>
  <td>--encoding ENCODE</td>
  <td>set the charset encoding (default: ascii)</td></tr>
  <tr><td></td>
  <td>--encodingmode MODE</td>
  <td>set the charset encoding mode (default: strict)</td></tr>
  <tr><td></td>
  <td>--traceback</td>
  <td>always print a traceback on exception</td></tr>
  <tr><td></td>
  <td>--time</td>
  <td>time how long the command takes</td></tr>
  <tr><td></td>
  <td>--profile</td>
  <td>print command execution profile</td></tr>
  <tr><td></td>
  <td>--version</td>
  <td>output version information and exit</td></tr>
  <tr><td>-h</td>
  <td>--help</td>
  <td>display help and exit</td></tr>
  <tr><td></td>
  <td>--hidden</td>
  <td>consider hidden changesets</td></tr>
  </table>
  <p>
  [+] marked option can be specified multiple times
  </p>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ "$TESTDIR/get-with-headers.py" 127.0.0.1:$HGPORT "help/revisions"
  200 Script output follows
  
  <!DOCTYPE html PUBLIC "-//W3C//DTD XHTML 1.1//EN" "http://www.w3.org/TR/xhtml11/DTD/xhtml11.dtd">
  <html xmlns="http://www.w3.org/1999/xhtml" xml:lang="en-US">
  <head>
  <link rel="icon" href="/static/hgicon.png" type="image/png" />
  <meta name="robots" content="index, nofollow" />
  <link rel="stylesheet" href="/static/style-paper.css" type="text/css" />
  <script type="text/javascript" src="/static/mercurial.js"></script>
  
  <title>Help: revisions</title>
  </head>
  <body>
  
  <div class="container">
  <div class="menu">
  <div class="logo">
  <a href="http://mercurial.selenic.com/">
  <img src="/static/hglogo.png" alt="mercurial" /></a>
  </div>
  <ul>
  <li><a href="/shortlog">log</a></li>
  <li><a href="/graph">graph</a></li>
  <li><a href="/tags">tags</a></li>
  <li><a href="/bookmarks">bookmarks</a></li>
  <li><a href="/branches">branches</a></li>
  </ul>
  <ul>
   <li class="active"><a href="/help">help</a></li>
  </ul>
  </div>
  
  <div class="main">
  <h2 class="breadcrumb"><a href="/">Mercurial</a> </h2>
  <h3>Help: revisions</h3>
  
  <form class="search" action="/log">
  
  <p><input name="rev" id="search1" type="text" size="30" /></p>
  <div id="hint">Find changesets by keywords (author, files, the commit message), revision
  number or hash, or <a href="/help/revsets">revset expression</a>.</div>
  </form>
  <div id="doc">
  <h1>Specifying Single Revisions</h1>
  <p>
  Mercurial supports several ways to specify individual revisions.
  </p>
  <p>
  A plain integer is treated as a revision number. Negative integers are
  treated as sequential offsets from the tip, with -1 denoting the tip,
  -2 denoting the revision prior to the tip, and so forth.
  </p>
  <p>
  A 40-digit hexadecimal string is treated as a unique revision
  identifier.
  </p>
  <p>
  A hexadecimal string less than 40 characters long is treated as a
  unique revision identifier and is referred to as a short-form
  identifier. A short-form identifier is only valid if it is the prefix
  of exactly one full-length identifier.
  </p>
  <p>
  Any other string is treated as a bookmark, tag, or branch name. A
  bookmark is a movable pointer to a revision. A tag is a permanent name
  associated with a revision. A branch name denotes the tipmost revision
  of that branch. Bookmark, tag, and branch names must not contain the &quot;:&quot;
  character.
  </p>
  <p>
  The reserved name &quot;tip&quot; always identifies the most recent revision.
  </p>
  <p>
  The reserved name &quot;null&quot; indicates the null revision. This is the
  revision of an empty repository, and the parent of revision 0.
  </p>
  <p>
  The reserved name &quot;.&quot; indicates the working directory parent. If no
  working directory is checked out, it is equivalent to null. If an
  uncommitted merge is in progress, &quot;.&quot; is the revision of the first
  parent.
  </p>
  
  </div>
  </div>
  </div>
  
  <script type="text/javascript">process_dates()</script>
  
  
  </body>
  </html>
  

  $ "$TESTDIR/killdaemons.py" $DAEMON_PIDS

#endif
