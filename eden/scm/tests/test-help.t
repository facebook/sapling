#chg-compatible
#require no-fsmonitor

Short help:

  $ hg
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
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
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files in the working directory
   web           launch Sapling Web GUI on localhost
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

  $ hg -q
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
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
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files in the working directory
   web           launch Sapling Web GUI on localhost
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

  $ hg help
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
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
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files in the working directory
   web           launch Sapling Web GUI on localhost
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

  $ hg -q help
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
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
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files in the working directory
   web           launch Sapling Web GUI on localhost
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates

Test extension help:
  $ hg help extensions --config extensions.rebase= --config extensions.children=
  Using Additional Features
  """""""""""""""""""""""""
  
      Mercurial has the ability to add new features through the use of
      extensions. Extensions may add new commands, add options to existing
      commands, change the default behavior of commands, or implement hooks.
  
      To enable the "foo" extension, either shipped with Mercurial or in the
      Python search path, create an entry for it in your configuration file,
      like this:
  
        [extensions]
        foo =
  
      You may also specify the full path to an extension:
  
        [extensions]
        myfeature = ~/.ext/myfeature.py
  
      See 'hg help config' for more information on configuration files.
  
      Extensions are not loaded by default for a variety of reasons: they can
      increase startup overhead; they may be meant for advanced usage only; they
      may provide potentially dangerous abilities (such as letting you destroy
      or modify history); they might not be ready for prime time; or they may
      alter some usual behaviors of stock Mercurial. It is thus up to the user
      to activate extensions as needed.
  
      To explicitly disable an extension enabled in a configuration file of
      broader scope, prepend its path with !:
  
        [extensions]
        # disabling extension bar residing in /path/to/extension/bar.py
        bar = !/path/to/extension/bar.py
        # ditto, but no path was supplied for extension baz
        baz = !
  
      Enabled extensions:
  
       conflictinfo
       debugshell    a python shell with repo, changelog & manifest objects
       errorredirect
                     redirect error message
       githelp       try mapping git commands to Mercurial commands
       lz4revlog     store revlog deltas using lz4 compression
       mergedriver   custom merge drivers for autoresolved files
       progressfile  allows users to have JSON progress bar information written
                     to a path
       rebase        command to move sets of revisions to a different ancestor
       eden          accelerated hg functionality in Eden checkouts (eden !)
       remotefilelog
                     minimize and speed up large repositories
       sampling      (no help text available)
       treemanifest
  
      Disabled extensions:
  
       absorb        apply working directory changes to changesets
       amend         extends the existing commit amend functionality
       arcdiff       (no help text available)
       blackbox      log repository events to a blackbox for debugging
       catnotate     (no help text available)
       checkmessagehook
                     (no help text available)
       checkserverbookmark
                     (no help text available)
       chistedit
       clienttelemetry
                     provide information about the client in server telemetry
       clonebundles  advertise pre-generated bundles to seed clones
       commitcloud   back up and sync changesets via the cloud
       copytrace     extension that does copytracing fast
       crdump        (no help text available)
       debugcommitmessage
                     (no help text available)
       debugnetwork  test network connections to the server
       dialect       replace terms with more widely used equivalents
       dirsync
       disablesymlinks
                     disables symlink support when enabled
       drop          drop specified changeset from the stack
       extdiff       command to allow external programs to compare revisions
       extorder
       extutil       (no help text available)
       fastannotate  yet another annotate implementation that might be faster
       fastlog
       fbhistedit    extends the existing histedit functionality
       fbscmquery    (no help text available)
       generic_bisect
                     (no help text available)
       gitrevset     map a git hash to a Mercurial hash:
       globalrevs    extension for providing strictly increasing revision
                     numbers
       grpcheck      check if the user is in specified groups
       hgevents      publishes state-enter and state-leave events to Watchman
       hgsql         sync hg repos with MySQL
       histedit      interactive history editing
       infinitepush  store draft commits in the cloud
       infinitepushbackup
                     back up draft commits in the cloud
       interactiveui
                     (no help text available)
       logginghelper
                     this extension logs different pieces of information that
                     will be used
       memcommit     make commits without a working copy
       morestatus    make status give a bit more context
       myparent
       ownercheck    prevent operations on repos not owned by the current user
       phabdiff      (no help text available)
       phabricator   simple Phabricator integration
       phabstatus    (no help text available)
       phrevset      provides support for Phabricator revsets
       preventpremegarepoupdateshook
                     (no help text available)
       prmarker      mark pull requests as "Landed" on pull
       pullcreatemarkers
       pushrebase    rebases commits during push
       rage          upload useful diagnostics and give instructions for asking
                     for help
       remotenames   mercurial extension for improving client/server workflows
       reset         reset the active bookmark and working copy to a desired
                     revision
       schemes       extend schemes with shortcuts to repository swarms
       share         share a common history between several working directories
       shelve        save and restore changes to the working directory
       sigtrace      sigtrace - dump stack and memory traces on signal
       simplecache
       smartlog      command to display a relevant subgraph
       snapshot      stores snapshots of uncommitted changes
       sparse        allow sparse checkouts of the working directory
       sshaskpass    ssh-askpass implementation that works with chg
       stablerev     provide a way to expose the "stable" commit via a revset
       traceprof     (no help text available)
       treemanifestserver
       tweakdefaults
                     user friendly defaults
       undo          (no help text available)
       win32mbcs     allow the use of MBCS paths with problematic encodings



Verify that extension keywords appear in help templates

  $ hg help --config extensions.phabdiff= templating|grep phabdiff > /dev/null

Normal help for add

  $ hg add -h
  hg add [OPTION]... [FILE]...
  
  start tracking the specified files
  
      Specify files to be tracked by Mercurial. The files will be added to the
      repository at the next commit.
  
      To undo an add before files have been committed, use 'hg forget'. To undo
      an add after files have been committed, use 'hg rm'.
  
      If no names are given, add all files to the repository (except files
      matching ".gitignore").
  
      Returns 0 if all files are successfully added.
  
  Options ([+] can be repeated):
  
   -I --include PATTERN [+] include files matching the given patterns
   -X --exclude PATTERN [+] exclude files matching the given patterns
   -n --dry-run             do not perform actions, just print output
  
  (some details hidden, use --verbose to show complete help)

Verbose help for add

  $ hg add -hv
  hg add [OPTION]... [FILE]...
  
  start tracking the specified files
  
      Specify files to be tracked by Mercurial. The files will be added to the
      repository at the next commit.
  
      To undo an add before files have been committed, use 'hg forget'. To undo
      an add after files have been committed, use 'hg rm'.
  
      If no names are given, add all files to the repository (except files
      matching ".gitignore").
  
      Examples:
  
        - New (unknown) files are added automatically by 'hg add':
  
            $ ls
            foo.c
            $ hg status
            ? foo.c
            $ hg add
            adding foo.c
            $ hg status
            A foo.c
  
        - Add specific files:
  
            $ ls
            bar.c  foo.c
            $ hg status
            ? bar.c
            ? foo.c
            $ hg add bar.c
            $ hg status
            A bar.c
            ? foo.c
  
      Returns 0 if all files are successfully added.
  
  Options ([+] can be repeated):
  
   -I --include PATTERN [+] include files matching the given patterns
   -X --exclude PATTERN [+] exclude files matching the given patterns
   -n --dry-run             do not perform actions, just print output
  
  Global options ([+] can be repeated):
  
   -R --repository REPO       repository root directory or name of overlay
                              bundle file
      --cwd DIR               change working directory
   -y --noninteractive        do not prompt, automatically pick the first choice
                              for all prompts
   -q --quiet                 suppress output
   -v --verbose               enable additional output
      --color TYPE            when to colorize (boolean, always, auto, never, or
                              debug)
      --config CONFIG [+]     set/override config option (use
                              'section.name=value')
      --configfile FILE [+]   enables the given config file
      --debug                 enable debugging output
      --debugger              start debugger
      --encoding ENCODE       set the charset encoding (default: utf-8)
      --encodingmode MODE     set the charset encoding mode (default: strict)
      --insecure              do not verify server certificate
      --outputencoding ENCODE set the output encoding (default: utf-8)
      --traceback             always print a traceback on exception
      --trace                 enable more detailed tracing
      --time                  time how long the command takes
      --profile               print command execution profile
      --version               output version information and exit
   -h --help                  display help and exit
      --hidden                consider hidden changesets
      --pager TYPE            when to paginate (boolean, always, auto, or never)
                              (default: auto)

Test the textwidth config option

  $ hg root -h  --config ui.textwidth=50
  hg root
  
  print the repository's root (top) of the current
  working directory
  
      Print the root directory of the current
      repository.
  
      Frequently useful in shells scripts and
      automation to run commands like:
  
        $  ./$(sl root)/bin/script.py
  
      Returns 0 on success.
  
  Options:
  
    --shared show root of the shared repo
  
  (some details hidden, use --verbose to show
  complete help)
Test help on a self-referencing alias that is a rust command

  $ hg --config "alias.root=root --shared" help root
  alias for: root --shared
  
  hg root
  
  print the repository's root (top) of the current working directory
  
      Print the root directory of the current repository.
  
      Frequently useful in shells scripts and automation to run commands like:
  
        $  ./$(sl root)/bin/script.py
  
      Returns 0 on success.
  
  Options:
  
    --shared show root of the shared repo
  
  (some details hidden, use --verbose to show complete help)
  $ hg --config "alias.root=root --shared" root -h
  alias for: root --shared
  
  hg root
  
  print the repository's root (top) of the current working directory
  
      Print the root directory of the current repository.
  
      Frequently useful in shells scripts and automation to run commands like:
  
        $  ./$(sl root)/bin/script.py
  
      Returns 0 on success.
  
  Options:
  
    --shared show root of the shared repo
  
  (some details hidden, use --verbose to show complete help)

Test help option with version option

  $ hg add -h --version
  Mercurial * (glob)

  $ hg add --skjdfks
  hg add: option --skjdfks not recognized
  (use 'hg add -h' to get help)
  [255]

Test ambiguous command help

  $ hg help ad
  abort: no such help topic: ad
  (try 'hg help --keyword ad')
  [255]

Test command without options

  $ hg help verify
  hg verify
  
  verify the integrity of the repository
  
      This command is a no-op.
  
  Options:
  
    --dag perform slower commit graph checks with server
  
  (some details hidden, use --verbose to show complete help)

  $ hg help diff
  hg diff [OPTION]... ([-c REV] | [-r REV1 [-r REV2]]) [FILE]...
  
  aliases: d
  
  show differences between commits
  
      Show the differences between two commits. If only one commit is specified,
      show the differences between the specified commit and your working copy.
      If no commits are specified, show your pending changes.
  
      Specify "-c" to see the changes in the specified commit relative to its
      parent.
  
      By default, this command skips binary files. To override this behavior,
      specify "-a" to include binary files in the diff.
  
      By default, diffs are shown using the unified diff format. Specify "-g" to
      generate diffs in the git extended diff format. For more information, see
      'hg help diffs'.
  
      Note:
         'hg diff' might generate unexpected results during merges because it
         defaults to comparing against your working copy's first parent commit
         if no commits are specified.
  
      Returns 0 on success.
  
  Options ([+] can be repeated):
  
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -a --text                treat all files as text
   -g --git                 use git extended diff format
      --binary              generate binary diffs in git mode (default)
      --nodates             omit dates from diff headers
      --noprefix            omit a/ and b/ prefixes from filenames
   -p --show-function       show which function each change is in
      --reverse             produce a diff that undoes the changes
   -w --ignore-all-space    ignore white space when comparing lines
   -b --ignore-space-change ignore changes in the amount of white space
   -B --ignore-blank-lines  ignore changes whose lines are all blank
   -Z --ignore-space-at-eol ignore changes in whitespace at EOL
   -U --unified NUM         number of lines of context to show
      --stat                output diffstat-style summary of changes
      --root DIR            produce diffs relative to subdirectory
      --only-files-in-revs  only show changes for files modified in the
                            requested revisions
   -I --include PATTERN [+] include files matching the given patterns
   -X --exclude PATTERN [+] exclude files matching the given patterns
  
  (some details hidden, use --verbose to show complete help)

  $ hg help status
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  list files with pending changes
  
      Show status of files in the working copy using the following status
      indicators:
  
        M = modified
        A = added
        R = removed
        C = clean
        ! = missing (deleted by a non-hg command, but still tracked)
        ? = not tracked
        I = ignored
          = origin of the previous file (with --copies)
  
      By default, shows files that have been modified, added, removed, deleted,
      or that are unknown (corresponding to the options "-mardu", respectively).
      Files that are unmodified, ignored, or the source of a copy/move operation
      are not listed.
  
      To control the exact statuses that are shown, specify the relevant flags
      (like "-rd" to show only files that are removed or deleted). Additionally,
      specify "-q/--quiet" to hide both unknown and ignored files.
  
      To show the status of specific files, provide a list of files to match. To
      include or exclude files using patterns or filesets, use "-I" or "-X".
  
      If "--rev" is specified and only one revision is given, it is used as the
      base revision. If two revisions are given, the differences between them
      are shown. The "--change" option can also be used as a shortcut to list
      the changed files of a revision from its first parent.
  
      Note:
         'hg status' might appear to disagree with 'hg diff' if permissions have
         changed or a merge has occurred, because the standard diff format does
         not report permission changes and 'hg diff' only reports changes
         relative to one merge parent.
  
      Returns 0 on success.
  
  Options ([+] can be repeated):
  
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
      --root-relative       show status relative to root
   -I --include PATTERN [+] include files matching the given patterns
   -X --exclude PATTERN [+] exclude files matching the given patterns
  
  (some details hidden, use --verbose to show complete help)

  $ hg -q help status
  hg status [OPTION]... [FILE]...
  
  list files with pending changes

  $ hg help foo
  abort: no such help topic: foo
  (try 'hg help --keyword foo')
  [255]

  $ hg skjdfks
  unknown command 'skjdfks'
  (use 'hg help' to get help)
  [255]

Typoed command gives suggestion
  $ hg puls
  unknown command 'puls'
  (use 'hg help' to get help)
  [255]

Not enabled extension gets suggested

  $ hg rebase
  unknown command 'rebase'
  (use 'hg help' to get help)
  [255]

Disabled extension gets suggested
  $ hg --config extensions.rebase=! rebase
  unknown command 'rebase'
  (use 'hg help' to get help)
  [255]

Make sure that we don't run afoul of the help system thinking that
this is a section and erroring out weirdly.

  $ hg .log
  unknown command '.log'
  (use 'hg help' to get help)
  [255]

  $ hg log.
  unknown command 'log.'
  (use 'hg help' to get help)
  [255]
  $ hg pu.lh
  unknown command 'pu.lh'
  (use 'hg help' to get help)
  [255]

  $ cat > helpext.py <<EOF
  > import os
  > from edenscm import commands, registrar
  > 
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > @command('nohelp',
  >     [('', 'longdesc', 3, 'x'*90),
  >     ('n', '', None, 'normal desc'),
  >     ('', 'newline', '', 'line1\nline2')],
  >     'hg nohelp',
  >     norepo=True)
  > @command('debugoptADV', [('', 'aopt', None, 'option is (ADVANCED)')])
  > @command('debugoptDEP', [('', 'dopt', None, 'option is (DEPRECATED)')])
  > @command('debugoptEXP', [('', 'eopt', None, 'option is (EXPERIMENTAL)')])
  > def nohelp(ui, *args, **kwargs):
  >     pass
  > 
  > def uisetup(ui):
  > 
  >     ui.setconfig('alias', 'shellalias', '!echo hi', 'helpext')
  >     ui.setconfig('alias', 'hgalias', 'summary', 'helpext')
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "helpext = `pwd`/helpext.py" >> $HGRCPATH

Test for aliases

  $ hg help hgalias
  alias for: summary
  
  hg summary [--remote]
  
  aliases: su
  
  summarize working directory state
  
      This generates a brief summary of the working directory state, including
      parents, branch, commit status, phase and available updates.
  
      With the --remote option, this will check the default paths for incoming
      and outgoing changes. This can be time-consuming.
  
      Returns 0 on success.
  
  Options:
  
    --remote check for push and pull
  
  (some details hidden, use --verbose to show complete help)

  $ hg help shellalias
  alias for: debugrunshell --cmd=echo hi
  
  hg debugrunshell
  
  run a shell command
  
  Options:
  
    --cmd VALUE command to run
  
  (some details hidden, use --verbose to show complete help)

Test command with no help text

  $ hg help nohelp
  hg nohelp
  
  (no help text available)
  
  Options:
  
      --longdesc VALUE xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx
                       xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx (default: 3)
   -n --               normal desc
      --newline VALUE  line1 line2
  
  (some details hidden, use --verbose to show complete help)

  $ hg help -k nohelp
  Commands:
  
   nohelp hg nohelp
  
  Extension Commands:
  
   nohelp (no help text available)

Commands in disabled extensions gets suggested even if there is no help text
for the module itself.

  $ hg help --config 'extensions.helpext=!'`pwd`/helpext.py nohelp
  'nohelp' is provided by the following extension:
  
      helpext       (no help text available)
  
  (use 'hg help extensions' for information on enabling extensions)

Test that default list of commands omits extension commands

  $ hg help
  Mercurial Distributed SCM
  
  hg COMMAND [OPTIONS]
  
  These are some common Mercurial commands.  Use 'hg help commands' to list all
  commands, and 'hg help COMMAND' to get help on a specific command.
  
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
  
  Rearrange commits:
  
   graft         copy commits from a different location
  
  Undo changes:
  
   uncommit      uncommit part or all of the current commit
  
  Other commands:
  
   config        show config settings
   doctor        attempt to check and fix issues
   grep          search for a pattern in tracked files in the working directory
   web           launch Sapling Web GUI on localhost
  
  Additional help topics:
  
   filesets      specifying files by their characteristics
   glossary      common terms
   patterns      specifying files by file name pattern
   revisions     specifying commits
   templating    customizing output with templates


Test list of internal help commands

  $ hg help debug
  Debug commands (internal and unsupported):
  
   debug-args    print arguments received
   debugancestor
                 find the ancestor revision of two revisions in a given index
   debugapi      send an EdenAPI request and print its output
   debugapplystreamclonebundle
                 apply a stream clone bundle file
   debugbenchmarkrevsets
                 benchmark revsets
   debugbindag   serialize dag to a compat binary format
   debugbuilddag
                 builds a repo with a given DAG from scratch in the current
                 empty repo
   debugbundle   lists the contents of a bundle
   debugcapabilities
                 lists the capabilities of a remote peer
   debugchangelog
                 show or migrate changelog backend
   debugcheckcasecollisions
                 check for case collisions against a commit
   debugcheckoutidentifier
                 display the current checkout unique identifier
   debugcheckstate
                 validate the correctness of the current dirstate
   debugcleanremotenames
                 remove non-essential remote bookmarks
   debugcolor    show available color, effects or style
   debugcommands
                 list all available commands and options
   debugcompactmetalog
                 compact the metalog by dropping history
   debugcomplete
                 returns the completion list associated with the given command
   debugcreatestreamclonebundle
                 create a stream clone bundle file
   debugdag      format the changelog or an index DAG as a concise textual
                 description
   debugdata     dump the contents of a data file revision
   debugdatapack
                 (no help text available)
   debugdate     parse and display a date
   debugdeltachain
                 dump information about delta chains in a revlog
   debugdetectissues
                 various repository integrity and health checks. for automatic
                 remediation, use doctor.
   debugdiffdirs
                 print the changed directories between two commits
   debugdifftree
                 diff two trees
   debugdirs     list directories
   debugdirstate
                 show the contents of the current dirstate
   debugdiscovery
                 runs the changeset discovery protocol in isolation
   debugdrawdag  read an ASCII graph from stdin and create changesets
   debugdryup    Execute native checkout (update) without actually writing to
                 working copy
   debugdumpdynamicconfig
                 print the dynamic configuration
   debugdumpindexedlog
                 dump indexedlog data
   debugdumptrace
                 export tracing information
   debugduplicatedconfig
                 find duplicated or overridden configs
   debugdynamicconfig
                 generate the dynamic configuration
   debugedenimporthelper
                 Obtain data for edenfs
   debugedenrunpostupdatehook
                 Run post-update hooks for edenfs
   debugexistingcasecollisions
                 check for existing case collisions in a commit
   debugexportmetalog
                 export metalog to a repo for easier investigation
   debugexportrevlog
                 exports to a legacy revlog repo
   debugextensions
                 show information about active extensions
   debugfilerevision
                 dump internal metadata for given file revisions
   debugfileset  parse and apply a fileset specification
   debugfsinfo   show information detected about current filesystem
   debugfsync    call fsync on newly modified key storage files
   debuggetbundle
                 retrieves a bundle from a repo
   debuggetroottree
                 (no help text available)
   debughistorypack
                 (no help text available)
   debughttp     check whether the EdenAPI server is reachable
   debugignore   display the combined ignore pattern and information about
                 ignored files
   debugindex    dump the contents of an index file
   debugindexdot
                 dump an index DAG as a graphviz dot file
   debugindexedlogdatastore
                 (no help text available)
   debugindexedloghistorystore
                 (no help text available)
   debuginitgit  init a repo from a git backend
   debuginstall  test Mercurial installation
   debuginternals
                 list or export internal files
   debugknown    test whether node ids are known to a repo
   debuglocks    show or modify state of locks
   debugmakepublic
                 make revisions public
   debugmanifestdirs
                 print treemanifest id, and paths
   debugmergestate
                 print merge state
   debugmetalog  show changes in commit graph over time
   debugmetalogroots
                 list roots stored in metalog
   debugmutation
                 display the mutation history (or future) of a commit
   debugmutationfromobsmarkers
                 convert obsolescence markers to mutation records
   debugnamecomplete
                 complete "names" - tags, open branch names, bookmark names
   debugnetworkdoctor
                 run the (Rust) network doctor
   debugobsolete
                 create arbitrary obsolete marker
   debugoptADV   (no help text available)
   debugoptDEP   (no help text available)
   debugoptEXP   (no help text available)
   debugpathcomplete
                 complete part or all of a tracked path
   debugpickmergetool
                 examine which merge tool is chosen for specified file
   debugpreviewbindag
                 print dag generated by debugbindag
   debugprocesstree
                 show process tree related to hg
   debugprogress
                 (no help text available)
   debugpull     test repo.pull interface
   debugpushkey  access the pushkey key/value protocol
   debugpvec     (no help text available)
   debugpython   run python interpreter
   debugracyoutput
                 exercise racy stdout / stderr / progress outputs
   debugreadauthforuri
                 (no help text available)
   debugrebuildchangelog
                 rebuild changelog by recloning and copying draft commits
   debugrebuilddirstate
                 rebuild the dirstate as it would look like for the given
                 revision
   debugrebuildfncache
                 rebuild the fncache file
   debugremotefilelog
                 (no help text available)
   debugrename   dump rename information
   debugresetheads
                 reset heads of repo so it looks like after a fresh clone
   debugrevlog   show data and statistics about a revlog
   debugrevlogclone
                 download revlog and bookmarks into a newly initialized repo
   debugrevset   resolves a single revset and outputs its commit hash
   debugrevspec  parse and apply a revision specification
   debugrunlog   display runlog entries
   debugrunshell
                 run a shell command
   debugruntest  run .t or Python doctest test
   debugscmstore
                 test file and tree fetching using scmstore
   debugscmstorereplay
                 replay scmstore activity log
   debugsegmentclone
                 clone a repository using segmented changelog
   debugsegmentgraph
                 display segment graph for a given group and level
   debugsegmentpull
                 pull a repository using segmented changelog. This command does
                 not do discovery and requrires specifying old/new master
                 revisions
   debugsendunbundle
                 Send unbundle wireproto command to a given server
   debugsetparents
                 manually set the parents of the current working directory
   debugshell    (no help text available)
   debugsmallcommitmetadata
                 store string metadata for a commit
   debugssl      test a secure connection to a server
   debugstatus   common performance issues for status
   debugstore    print information about blobstore
   debugstrip    strip commits and all their descendants from the repository
   debugsuccessorssets
                 show set of successors for revision
   debugtemplate
                 parse and apply a template
   debugthrowexception
                 cause an intentional exception to be raised in the command
   debugthrowrustbail
                 cause an error to be returned from rust and propagated to
                 python using bail
   debugthrowrustexception
                 cause an error to be returned from rust and propagated to
                 python
   debugtop      outputs information about all running commands for the current
                 repository
   debugtreestate
                 manage treestate
   debugupdatecaches
                 warm all known caches in the repository
   debugvisibility
                 control visibility tracking
   debugvisibleheads
                 print visible heads
   debugwaitonprefetch
                 (no help text available)
   debugwaitonrepack
                 (no help text available)
   debugwalk     show how files match on given patterns
   debugwireargs
                 (no help text available)

Test list of commands with command with no help text

  $ hg help helpext
  helpext extension - no help text available
  
  Commands:
  
   nohelp        (no help text available)


test advanced, deprecated and experimental options are hidden in command help
  $ hg help debugoptADV
  hg debugoptADV
  
  (no help text available)
  
  (some details hidden, use --verbose to show complete help)
  $ hg help debugoptDEP
  hg debugoptDEP
  
  (no help text available)
  
  (some details hidden, use --verbose to show complete help)

  $ hg help debugoptEXP
  hg debugoptEXP
  
  (no help text available)
  
  (some details hidden, use --verbose to show complete help)

test advanced, deprecated and experimental options are shown with -v
  $ hg help -v debugoptADV | grep aopt
    --aopt option is (ADVANCED)
  $ hg help -v debugoptDEP | grep dopt
    --dopt option is (DEPRECATED)
  $ hg help -v debugoptEXP | grep eopt
    --eopt option is (EXPERIMENTAL)

#if gettext normal-layout
test deprecated option is hidden with translation with untranslated description
(use many globy for not failing on changed transaction)
  $ LANGUAGE=sv hg help debugoptDEP
  hg debugoptDEP
  
  (*) (glob)
  
  (some details hidden, use --verbose to show complete help)
#endif

Test commands that collide with topics (issue4240)

  $ hg config -hq
  hg config [OPTION]... [NAME]...
  
  show config settings
  $ hg showconfig -hq
  hg config [OPTION]... [NAME]...
  
  show config settings

Test a help topic

  $ hg help dates
  Date Formats
  """"""""""""
  
      Some commands allow the user to specify a date, e.g.:
  
      - backout, commit, import, tag: Specify the commit date.
      - log, revert, update: Select revision(s) by date.
  
      Many date formats are valid. Here are some examples:
  
      - "Wed Dec 6 13:18:29 2006" (local timezone assumed)
      - "Dec 6 13:18 -0600" (year assumed, time offset provided)
      - "Dec 6 13:18 UTC" (UTC and GMT are aliases for +0000)
      - "Dec 6" (midnight)
      - "13:18" (today assumed)
      - "3:39" (3:39AM assumed)
      - "3:39pm" (15:39)
      - "2006-12-06 13:18:29" (ISO 8601 format)
      - "2006-12-6 13:18"
      - "2006-12-6"
      - "12-6"
      - "12/6"
      - "12/6/6" (Dec 6 2006)
      - "today" (midnight)
      - "yesterday" (midnight)
      - "now" - right now
  
      Lastly, there is Mercurial's internal format:
  
      - "1165411109 0" (Wed Dec 6 13:18:29 2006 UTC)
  
      This is the internal representation format for dates. The first number is
      the number of seconds since the epoch (1970-01-01 00:00 UTC). The second
      is the offset of the local timezone, in seconds west of UTC (negative if
      the timezone is east of UTC).
  
      The log command also accepts date ranges:
  
      - "<DATE" - at or before a given date/time
      - ">DATE" - on or after a given date/time
      - "DATE to DATE" - a date range, inclusive
      - "-DAYS" - within a given number of days of today

Test repeated config section name

  $ hg help config.host
      "http_proxy.host"
          Host name and (optional) port of the proxy server, for example
          "myproxy:8000".
  
      "smtp.host"
          Host name of mail server, e.g. "mail.example.com".
  
Unrelated trailing paragraphs shouldn't be included

  $ hg help config.extramsg | grep '^$'
  

Test capitalized section name

  $ hg help scripting.HGPLAIN > /dev/null

Help subsection:

  $ hg help config.charsets |grep "Email example:" > /dev/null
  [1]

Show nested definitions
("profiling.type"[break]"ls"[break]"stat"[break])

  $ hg help config.type | egrep '^$'|wc -l
  \s*3 (re)

Separate sections from subsections

  $ hg help config.format | egrep '^    ("|-)|^\s*$' | uniq
      "format"
      --------
  
      "usegeneraldelta"
  
      "dirstate"
  
      "uselz4"
  
      "cgdeltabase"
  
      "profiling"
      -----------
  
      "format"
  
      "progress"
      ----------
  
      "format"
  

Last item in help config.*:

  $ hg help config.`hg help config|grep '^    "'| \
  >       tail -1|sed 's![ "]*!!g'`| \
  >   grep 'hg help -c config' > /dev/null
  [1]

note to use help -c for general hg help config:

  $ hg help config |grep 'hg help -c config' > /dev/null

Test templating help

  $ hg help templating | egrep '(desc|diffstat|firstline|nonempty)  '
      desc          String. The text of the changeset description.
      diffstat      String. Statistics of changes with the following format:
      firstline     Any text. Returns the first line of text.
      nonempty      Any text. Returns '(none)' if the string is empty.

Test deprecated items

  $ hg help -v templating | grep currentbookmark
      currentbookmark
  $ hg help templating | (grep currentbookmark || true)

Test help hooks

  $ cat > helphook1.py <<EOF
  > from edenscm import help
  > 
  > def rewrite(ui, topic, doc):
  >     return doc + '\nhelphook1\n'
  > 
  > def extsetup(ui):
  >     help.addtopichook('revisions', rewrite)
  > EOF
  $ cat > helphook2.py <<EOF
  > from edenscm import help
  > 
  > def rewrite(ui, topic, doc):
  >     return doc + '\nhelphook2\n'
  > 
  > def extsetup(ui):
  >     help.addtopichook('revisions', rewrite)
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "helphook1 = `pwd`/helphook1.py" >> $HGRCPATH
  $ echo "helphook2 = `pwd`/helphook2.py" >> $HGRCPATH
  $ hg help revsets | grep helphook
      helphook1
      helphook2

help -c should only show debug --debug

  $ hg help -c --debug|egrep debug|wc -l|egrep '^\s*0\s*$'
  [1]

help -c should only show deprecated for -v

  $ hg help -c -v|egrep DEPRECATED|wc -l|egrep '^\s*0\s*$'
  [1]

Test -s / --system

  $ hg help config.files -s windows |grep 'etc/mercurial' | \
  > wc -l | sed -e 's/ //g'
  0
  $ hg help config.files --system unix | grep 'USER' | \
  > wc -l | sed -e 's/ //g'
  0

Test -e / -c / -k combinations

  $ hg help -c|egrep '^[A-Z].*:|^ debug'
  Commands:
  $ hg help -e|egrep '^[A-Z].*:|^ debug'
  Extensions:
   debugcommitmessage            (no help text available)
   debugnetwork                  test network connections to the server
   debugshell                    a python shell with repo, changelog & manifest
  $ hg help -k|egrep '^[A-Z].*:|^ debug'
  Topics:
  Commands:
  Extensions:
   debugcommitmessage            (no help text available)
   debugnetwork                  test network connections to the server
   debugshell                    a python shell with repo, changelog & manifest
  Extension Commands:
  $ hg help -c -k dates |egrep '^(Topics|Extensions|Commands):'
  Commands:
  $ hg help -e -k a |egrep '^(Topics|Extensions|Commands):'
  Extensions:
  $ hg help -e -c -k date |egrep '^(Topics|Extensions|Commands):'
  Extensions:
  Commands:
  $ hg help -c commit > /dev/null
  $ hg help -e -c commit > /dev/null
  $ hg help -e commit > /dev/null
  abort: no such help topic: commit
  (try 'hg help --keyword commit')
  [255]

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
   glossary   Common Terms
   phases     Working with Phases
   urls       URL Paths
  
  Commands:
  
   bookmark create a new bookmark or list existing bookmarks
   clone    make a copy of an existing repository
   paths    show aliases for remote repositories
  
  Extensions:
  
   clonebundles advertise pre-generated bundles to seed clones
   prefixedname matched against word "clone"

Test unfound topic

  $ hg help nonexistingtopicthatwillneverexisteverever
  abort: no such help topic: nonexistingtopicthatwillneverexisteverever
  (try 'hg help --keyword nonexistingtopicthatwillneverexisteverever')
  [255]

Test unfound keyword

  $ hg help --keyword nonexistingwordthatwillneverexisteverever
  abort: no matches
  (try 'hg help' for a list of topics)
  [255]

Test omit indicating for help

  $ cat > addverboseitems.py <<EOF
  > '''extension to test omit indicating.
  > 
  > This paragraph is never omitted (for extension)
  > 
  > .. container:: verbose
  > 
  >   This paragraph is omitted,
  >   if :hg:\`help\` is invoked without \`\`-v\`\` (for extension)
  > 
  > This paragraph is never omitted, too (for extension)
  > '''
  > from __future__ import absolute_import
  > from edenscm import commands, help
  > testtopic = """This paragraph is never omitted (for topic).
  > 
  > .. container:: verbose
  > 
  >   This paragraph is omitted,
  >   if :hg:\`help\` is invoked without \`\`-v\`\` (for topic)
  > 
  > This paragraph is never omitted, too (for topic)
  > """
  > def extsetup(ui):
  >     help.helptable.append((["topic-containing-verbose"],
  >                            "This is the topic to test omit indicating.",
  >                            lambda ui: testtopic))
  > EOF
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "addverboseitems = `pwd`/addverboseitems.py" >> $HGRCPATH
  $ hg help addverboseitems
  addverboseitems extension - extension to test omit indicating.
  
  This paragraph is never omitted (for extension)
  
  This paragraph is never omitted, too (for extension)
  
  (some details hidden, use --verbose to show complete help)
  
  no commands defined
  $ hg help -v addverboseitems
  addverboseitems extension - extension to test omit indicating.
  
  This paragraph is never omitted (for extension)
  
  This paragraph is omitted, if 'hg help' is invoked without "-v" (for
  extension)
  
  This paragraph is never omitted, too (for extension)
  
  no commands defined
  $ hg help topic-containing-verbose
  This is the topic to test omit indicating.
  """"""""""""""""""""""""""""""""""""""""""
  
      This paragraph is never omitted (for topic).
  
      This paragraph is never omitted, too (for topic)
  
  (some details hidden, use --verbose to show complete help)
  $ hg help -v topic-containing-verbose
  This is the topic to test omit indicating.
  """"""""""""""""""""""""""""""""""""""""""
  
      This paragraph is never omitted (for topic).
  
      This paragraph is omitted, if 'hg help' is invoked without "-v" (for
      topic)
  
      This paragraph is never omitted, too (for topic)

Test section lookup

  $ hg help revset.merge
      "merge()"
        Changeset is a merge changeset.
  
  $ hg help glossary.dag
      DAG
          The repository of changesets of a distributed version control system
          (DVCS) can be described as a directed acyclic graph (DAG), consisting
          of nodes and edges, where nodes correspond to changesets and edges
          imply a parent -> child relation. This graph can be visualized by
          graphical tools such as 'hg log --graph'. In Mercurial, the DAG is
          limited by the requirement for children to have at most two parents.
  

  $ hg help hgrc.paths
      "paths"
      -------
  
      Assigns symbolic names and behavior to repositories.
  
      Options are symbolic names defining the URL or directory that is the
      location of the repository. Example:
  
        [paths]
        my_server = https://example.com/my_repo
        local_path = /home/me/repo
  
      These symbolic names can be used from the command line. To pull from
      "my_server": 'hg pull my_server'. To push to "local_path": 'hg push
      local_path'.
  
      Options containing colons (":") denote sub-options that can influence
      behavior for that specific path. Example:
  
        [paths]
        my_server = https://example.com/my_path
        my_server:pushurl = ssh://example.com/my_path
  
      The following sub-options can be defined:
  
      "pushurl"
         The URL to use for push operations. If not defined, the location
         defined by the path's main entry is used.
  
      "pushrev"
         A revset defining which revisions to push by default.
  
         When 'hg push' is executed without a "-r" argument, the revset defined
         by this sub-option is evaluated to determine what to push.
  
         For example, a value of "." will push the working directory's revision
         by default.
  
         Revsets specifying bookmarks will not result in the bookmark being
         pushed.
  
      The following special named paths exist:
  
      "default"
         The URL or directory to use when no source or remote is specified.
  
         'hg clone' will automatically define this path to the location the
         repository was cloned from.
  
      "default-push"
         (deprecated) The URL or directory for the default 'hg push' location.
         "default:pushurl" should be used instead.
  
  $ hg help glossary.mcguffin
  abort: help section not found: glossary.mcguffin
  [255]

  $ hg help glossary.mc.guffin
  abort: help section not found: glossary.mc.guffin
  [255]

  $ hg help template.files
      files         List of strings. All files modified, added, or removed by
                    this changeset.
      files(pattern)
                    All files of the current changeset matching the pattern. See
                    'hg help patterns'.

Test section lookup by translated message

str.lower() instead of encoding.lower(str) on translated message might
make message meaningless, because some encoding uses 0x41(A) - 0x5a(Z)
as the second or later byte of multi-byte character.

For example, "\x8bL\x98^" (translation of "record" in ja_JP.cp932)
contains 0x4c (L). str.lower() replaces 0x4c(L) by 0x6c(l) and this
replacement makes message meaningless.

This tests that section lookup by translated string isn't broken by
such str.lower().

  $ $PYTHON <<EOF
  > def escape(s):
  >     return ''.join('\\\u%x' % ord(uc) for uc in s.decode('cp932'))
  > # translation of "record" in ja_JP.cp932
  > upper = b"\x8bL\x98^"
  > # str.lower()-ed section name should be treated as different one
  > lower = b"\x8bl\x98^"
  > with open('ambiguous.py', 'w') as fp:
  >     fp.write("""# ambiguous section names in ja_JP.cp932
  > u'''summary of extension
  > 
  > %s
  > ----
  > 
  > Upper name should show only this message
  > 
  > %s
  > ----
  > 
  > Lower name should show only this message
  > 
  > subsequent section
  > ------------------
  > 
  > This should be hidden at 'hg help ambiguous' with section name.
  > '''
  > """ % (escape(upper), escape(lower)))
  > EOF

Show help content of disabled extensions

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > ambiguous = !./ambiguous.py
  > EOF
  $ hg help -e ambiguous
  ambiguous extension - (no help text available)
  
  (use 'hg help extensions' for information on enabling extensions)

Test dynamic list of merge tools only shows up once
  $ hg help merge-tools
  Merge Tools
  """""""""""
  
      To merge files Mercurial uses merge tools.
  
      A merge tool combines two different versions of a file into a merged file.
      Merge tools are given the two files and the greatest common ancestor of
      the two file versions, so they can determine the changes made on both
      branches.
  
      Merge tools are used both for 'hg resolve', 'hg merge', 'hg goto', 'hg
      backout' and in several extensions.
  
      Usually, the merge tool tries to automatically reconcile the files by
      combining all non-overlapping changes that occurred separately in the two
      different evolutions of the same initial base file. Furthermore, some
      interactive merge programs make it easier to manually resolve conflicting
      merges, either in a graphical way, or by inserting some conflict markers.
      Mercurial does not include any interactive merge programs but relies on
      external tools for that.
  
      Available merge tools
      =====================
  
      External merge tools and their properties are configured in the merge-
      tools configuration section - see 'hg help config.merge-tools' - but they
      can often just be named by their executable.
  
      A merge tool is generally usable if its executable can be found on the
      system and if it can handle the merge. The executable is found if it is an
      absolute or relative executable path or the name of an application in the
      executable search path. The tool is assumed to be able to handle the merge
      if it can handle symlinks if the file is a symlink, if it can handle
      binary files if the file is binary, and if a GUI is available if the tool
      requires a GUI.
  
      There are some internal merge tools which can be used. The internal merge
      tools are:
  
      ":dump"
        Creates three versions of the files to merge, containing the contents of
        local, other and base. These files can then be used to perform a merge
        manually. If the file to be merged is named "a.txt", these files will
        accordingly be named "a.txt.local", "a.txt.other" and "a.txt.base" and
        they will be placed in the same directory as "a.txt".
  
        This implies premerge. Therefore, files aren't dumped, if premerge runs
        successfully. Use :forcedump to forcibly write files out.
  
      ":fail"
        Rather than attempting to merge files that were modified on both
        branches, it marks them as unresolved. The resolve command must be used
        to resolve these conflicts.
  
      ":forcedump"
        Creates three versions of the files as same as :dump, but omits
        premerge.
  
      ":local"
        Uses the local 'p1()' version of files as the merged version.
  
      ":merge"
        Uses the internal non-interactive simple merge algorithm for merging
        files. It will fail if there are any conflicts and leave markers in the
        partially merged file. Markers will have two sections, one for each side
        of merge.
  
      ":merge-local"
        Like :merge, but resolve all conflicts non-interactively in favor of the
        local 'p1()' changes.
  
      ":merge-other"
        Like :merge, but resolve all conflicts non-interactively in favor of the
        other 'p2()' changes.
  
      ":merge3"
        Uses the internal non-interactive simple merge algorithm for merging
        files. It will fail if there are any conflicts and leave markers in the
        partially merged file. Marker will have three sections, one from each
        side of the merge and one for the base content.
  
      ":mergediff"
        Uses the internal non-interactive simple merge algorithm for merging
        files. It will fail if there are any conflicts and leave markers in the
        partially merged file. The marker will have two sections, one with the
        content from one side of the merge, and one with a diff from the base
        content to the content on the other side. (experimental)
  
      ":other"
        Uses the other 'p2()' version of files as the merged version.
  
      ":prompt"
        Asks the user which of the local 'p1()' or the other 'p2()' version to
        keep as the merged version.
  
      ":union"
        Uses the internal non-interactive simple merge algorithm for merging
        files. It will use both left and right sides for conflict regions. No
        markers are inserted.
  
      Internal tools are always available and do not require a GUI but will by
      default not handle symlinks or binary files.
  
      Choosing a merge tool
      =====================
  
      Mercurial uses these rules when deciding which merge tool to use:
  
      1. If a tool has been specified with the --tool option to merge or
         resolve, it is used.  If it is the name of a tool in the merge-tools
         configuration, its configuration is used. Otherwise the specified tool
         must be executable by the shell.
      2. If the "HGMERGE" environment variable is present, its value is used and
         must be executable by the shell.
      3. If the filename of the file to be merged matches any of the patterns in
         the merge-patterns configuration section, the first usable merge tool
         corresponding to a matching pattern is used. Here, binary capabilities
         of the merge tool are not considered.
      4. If ui.merge is set it will be considered next. If the value is not the
         name of a configured tool, the specified value is used and must be
         executable by the shell. Otherwise the named tool is used if it is
         usable.
      5. If any usable merge tools are present in the merge-tools configuration
         section, the one with the highest priority is used.
      6. If a program named "hgmerge" can be found on the system, it is used -
         but it will by default not be used for symlinks and binary files.
      7. If the file to be merged is not binary and is not a symlink, then
         internal ":merge" is used.
      8. Otherwise, ":prompt" is used.
  
      Note:
         After selecting a merge program, Mercurial will by default attempt to
         merge the files using a simple merge algorithm first. Only if it
         doesn't succeed because of conflicting changes will Mercurial actually
         execute the merge program. Whether to use the simple merge algorithm
         first can be controlled by the premerge setting of the merge tool.
         Premerge is enabled by default unless the file is binary or a symlink.
  
      See the merge-tools and ui sections of 'hg help config' for details on the
      configuration of merge tools.

Compression engines listed in `hg help bundlespec`

  $ hg help bundlespec | grep gzip
          "v1" bundles can only use the "gzip", "bzip2", and "none" compression
        An algorithm that produces smaller bundles than "gzip".
        This engine will likely produce smaller bundles than "gzip" but will be
      "gzip"
        better compression than "gzip". It also frequently yields better (?)

#if normal-layout
Test usage of section marks in help documents

  $ cd "$TESTDIR"/../doc
  $ hg debugpython -- check-seclevel.py
#endif
