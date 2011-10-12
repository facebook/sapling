Short help:

  $ hg
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

  $ hg -q
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

  $ hg help
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

  $ hg -q help
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

Test short command list with verbose option

  $ hg -v help shortlist
  Mercurial Distributed SCM
  
  basic commands:
  
   add:
        add the specified files on the next commit
   annotate, blame:
        show changeset information by line for each file
   clone:
        make a copy of an existing repository
   commit, ci:
        commit the specified files or all outstanding changes
   diff:
        diff repository (or selected files)
   export:
        dump the header and diffs for one or more changesets
   forget:
        forget the specified files on the next commit
   init:
        create a new repository in the given directory
   log, history:
        show revision history of entire repository or files
   merge:
        merge working directory with another revision
   pull:
        pull changes from the specified source
   push:
        push changes to the specified destination
   remove, rm:
        remove the specified files on the next commit
   serve:
        start stand-alone webserver
   status, st:
        show changed files in the working directory
   summary, sum:
        summarize working directory state
   update, up, checkout, co:
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
  
  use "hg -v help add" to show more info

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
  
  [+] marked option can be specified multiple times

Test help option with version option

  $ hg add -h --version
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-2011 Matt Mackall and others
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
  
   add         add the specified files on the next commit
   addremove   add all new files, delete all missing files
  
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
  
      Returns 0 on success, 1 if errors are encountered.
  
  use "hg -v help verify" to show more info

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
  
  use "hg -v help diff" to show more info

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
  
  use "hg -v help status" to show more info

  $ hg -q help status
  hg status [OPTION]... [FILE]...
  
  show changed files in the working directory

  $ hg help foo
  hg: unknown command 'foo'
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

  $ hg skjdfks
  hg: unknown command 'skjdfks'
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
  
  use "hg -v help nohelp" to show more info

Test that default list of commands omits extension commands

  $ hg help
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
  
  enabled extensions:
  
   helpext  (no help text available)
  
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



Test list of commands with command with no help text

  $ hg help helpext
  helpext extension - no help text available
  
  list of commands:
  
   nohelp   (no help text available)
  
  use "hg -v help helpext" to show builtin aliases and global options

Test a help topic

  $ hg help revs
  Specifying Single Revisions
  
      Mercurial supports several ways to specify individual revisions.
  
      A plain integer is treated as a revision number. Negative integers are
      treated as sequential offsets from the tip, with -1 denoting the tip, -2
      denoting the revision prior to the tip, and so forth.
  
      A 40-digit hexadecimal string is treated as a unique revision identifier.
  
      A hexadecimal string less than 40 characters long is treated as a unique
      revision identifier and is referred to as a short-form identifier. A
      short-form identifier is only valid if it is the prefix of exactly one
      full-length identifier.
  
      Any other string is treated as a tag or branch name. A tag name is a
      symbolic name associated with a revision identifier. A branch name denotes
      the tipmost revision of that branch. Tag and branch names must not contain
      the ":" character.
  
      The reserved name "tip" is a special tag that always identifies the most
      recent revision.
  
      The reserved name "null" indicates the null revision. This is the revision
      of an empty repository, and the parent of revision 0.
  
      The reserved name "." indicates the working directory parent. If no
      working directory is checked out, it is equivalent to null. If an
      uncommitted merge is in progress, "." is the revision of the first parent.

Test templating help

  $ hg help templating | egrep '(desc|diffstat|firstline|nonempty)  '
      desc        String. The text of the changeset description.
      diffstat    String. Statistics of changes with the following format:
      firstline   Any text. Returns the first line of text.
      nonempty    Any text. Returns '(none)' if the string is empty.

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
