  $ setconfig extensions.treemanifest=!
  $ HGFOO=BAR; export HGFOO
  $ cat >> $HGRCPATH <<'EOF'
  > [alias]
  > # should clobber ci but not commit (issue2993)
  > ci = version
  > myinit = init
  > mycommit = commit
  > optionalrepo = showconfig alias.myinit
  > cleanstatus = status -c
  > unknown = bargle
  > ambiguous = s
  > recursive = recursive
  > disabled = purge
  > nodefinition =
  > noclosingquotation = '
  > no--cwd = status --cwd elsewhere
  > no-R = status -R elsewhere
  > no--repo = status --repo elsewhere
  > no--repository = status --repository elsewhere
  > no--config = status --config a.config=1
  > mylog = log
  > lognull = log -r null
  > shortlog = log --template '{rev} {node|short} | {date|isodate}\n'
  > positional = log --template '{$2} {$1} | {date|isodate}\n'
  > dln = lognull --debug
  > nousage = rollback
  > put = export -r 0 -o "$FOO/%R.diff"
  > blank = !echo
  > self = !echo $0
  > echoall = !echo "$@"
  > echo1 = !echo $1
  > echo2 = !echo $2
  > echo13 = !echo $1 $3
  > echotokens = !printf "%s\n" "$@"
  > count = !hg log -r "$@" --template=. | wc -c | sed -e 's/ //g'
  > mcount = !hg log $@ --template=. | wc -c | sed -e 's/ //g'
  > rt = root
  > idalias = id
  > idaliaslong = id
  > idaliasshell = !echo test
  > parentsshell1 = !echo one
  > parentsshell2 = !echo two
  > escaped1 = !echo 'test$$test'
  > escaped2 = !echo "HGFOO is $$HGFOO"
  > escaped3 = !echo $1 is $$$1
  > escaped4 = !echo \$$0 \$$@
  > exit1 = !sh -c 'exit 1'
  > documented = id
  > documented:doc = an alias for the id command
  > [defaults]
  > mylog = -q
  > lognull = -q
  > log = -v
  > EOF


basic

  $ hg myinit alias


unknown

  $ hg unknown
  unknown command 'bargle'
  (use 'hg help' to get help)
  [255]
  $ hg help unknown
  alias for: bargle
  
  abort: no such help topic: unknown
  (try 'hg help --keyword unknown')
  [255]


ambiguous

  $ hg ambiguous
  hg: command 's' is ambiguous:
  	self
  	serve
  	shortlog
  	show
  	showconfig
  	status
  	summary
  [255]
  $ hg help ambiguous
  alias for: s
  
  Commands:
  
   self          (no help text available)
   serve         start stand-alone webserver
   shortlog      show commit history
   show          show commit in detail
   status        list files with pending changes
   summary       summarize working directory state


recursive

  $ hg recursive
  abort: alias 'recursive' resolves to unknown command 'recursive'
  [255]
  $ hg help recursive
  alias 'recursive' resolves to unknown command 'recursive'


disabled

  $ hg disabled
  unknown command 'purge'
  (use 'hg help' to get help)
  [255]
  $ hg help disabled
  alias for: purge
  
  abort: no such help topic: disabled
  (try 'hg help --keyword disabled')
  [255]





no definition

  $ hg nodef
  abort: alias definition nodefinition = "" cannot be parsed
  [255]
  $ hg help nodef
  abort: alias definition nodefinition = "" cannot be parsed
  [255]


no closing quotation

  $ hg noclosing
  abort: alias definition noclosingquotation = "\'" cannot be parsed
  [255]
  $ hg help noclosing
  abort: alias definition noclosingquotation = "\'" cannot be parsed
  [255]

"--" in alias definition should be preserved

  $ hg --config alias.dash='cat --' -R alias dash -r0
  abort: -r0 not under root '$TESTTMP/alias'
  (consider using '--cwd alias')
  [255]

invalid options

  $ hg no--cwd
  abort: option --cwd may not be abbreviated!
  [255]
  $ hg help no--cwd
  alias for: status --cwd elsewhere
  
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  list files with pending changes
  
      Show status of files in the repository using the following status
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
      or that are unknown (corresponding to the options -mardu). Files that are
      unmodified, ignored, or the source of a copy/move operation are not
      listed.
  
      To control the exact statuses that are shown, specify the relevant flags
      (like -rd to show only files that are removed or deleted). Additionally,
      specify -q/--quiet to hide both unknown and ignored files.
  
      To show the status of specific files, provide an explicit list of files to
      match. To include or exclude files using regular expressions, use -I or
      -X.
  
      If --rev is specified, and only one revision is given, it is used as the
      base revision. If two revisions are given, the differences between them
      are shown. The --change option can also be used as a shortcut to list the
      changed files of a revision from its first parent.
  
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
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)
  $ hg no-R
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg help no-R
  alias for: status -R elsewhere
  
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  list files with pending changes
  
      Show status of files in the repository using the following status
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
      or that are unknown (corresponding to the options -mardu). Files that are
      unmodified, ignored, or the source of a copy/move operation are not
      listed.
  
      To control the exact statuses that are shown, specify the relevant flags
      (like -rd to show only files that are removed or deleted). Additionally,
      specify -q/--quiet to hide both unknown and ignored files.
  
      To show the status of specific files, provide an explicit list of files to
      match. To include or exclude files using regular expressions, use -I or
      -X.
  
      If --rev is specified, and only one revision is given, it is used as the
      base revision. If two revisions are given, the differences between them
      are shown. The --change option can also be used as a shortcut to list the
      changed files of a revision from its first parent.
  
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
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)
  $ hg no--repo
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg help no--repo
  alias for: status --repo elsewhere
  
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  list files with pending changes
  
      Show status of files in the repository using the following status
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
      or that are unknown (corresponding to the options -mardu). Files that are
      unmodified, ignored, or the source of a copy/move operation are not
      listed.
  
      To control the exact statuses that are shown, specify the relevant flags
      (like -rd to show only files that are removed or deleted). Additionally,
      specify -q/--quiet to hide both unknown and ignored files.
  
      To show the status of specific files, provide an explicit list of files to
      match. To include or exclude files using regular expressions, use -I or
      -X.
  
      If --rev is specified, and only one revision is given, it is used as the
      base revision. If two revisions are given, the differences between them
      are shown. The --change option can also be used as a shortcut to list the
      changed files of a revision from its first parent.
  
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
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)
  $ hg no--repository
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg help no--repository
  alias for: status --repository elsewhere
  
  hg status [OPTION]... [FILE]...
  
  aliases: st
  
  list files with pending changes
  
      Show status of files in the repository using the following status
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
      or that are unknown (corresponding to the options -mardu). Files that are
      unmodified, ignored, or the source of a copy/move operation are not
      listed.
  
      To control the exact statuses that are shown, specify the relevant flags
      (like -rd to show only files that are removed or deleted). Additionally,
      specify -q/--quiet to hide both unknown and ignored files.
  
      To show the status of specific files, provide an explicit list of files to
      match. To include or exclude files using regular expressions, use -I or
      -X.
  
      If --rev is specified, and only one revision is given, it is used as the
      base revision. If two revisions are given, the differences between them
      are shown. The --change option can also be used as a shortcut to list the
      changed files of a revision from its first parent.
  
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
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)
  $ hg no--config
  abort: option --config may not be abbreviated!
  [255]
  $ hg no --config alias.no='--repo elsewhere --cwd elsewhere status'
  unknown command '--repo'
  (use 'hg help' to get help)
  [255]
  $ hg no --config alias.no='--repo elsewhere'
  unknown command '--repo'
  (use 'hg help' to get help)
  [255]

optional repository

#if no-outer-repo
  $ hg optionalrepo
  init
#endif
  $ cd alias
  $ cat > .hg/hgrc <<EOF
  > [alias]
  > myinit = init -q
  > EOF
  $ hg optionalrepo
  init -q

no usage

  $ hg nousage
  no rollback information available
  [1]

  $ echo foo > foo
  $ hg commit -Amfoo
  adding foo

infer repository

  $ cd ..

#if no-outer-repo
  $ hg shortlog alias/foo
  0 e63c23eaa88a | 1970-01-01 00:00 +0000
#endif

  $ cd alias

with opts

  $ hg cleanst
  C foo


with opts and whitespace

  $ hg shortlog
  0 e63c23eaa88a | 1970-01-01 00:00 +0000

interaction with defaults

  $ hg mylog
  changeset:   0:e63c23eaa88a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo
  
  $ hg lognull
  changeset:   -1:000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  


properly recursive

  $ hg dln
  changeset:   -1:0000000000000000000000000000000000000000
  phase:       public
  parent:      -1:0000000000000000000000000000000000000000
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    0000000000000000000000000000000000000000
  user:        
  date:        Thu Jan 01 00:00:00 1970 +0000
  extra:       branch=default
  

simple shell aliases

  $ hg blank
  

  $ hg blank foo
  

  $ hg self
  self
  $ hg echoall
  

  $ hg echoall foo
  foo
  $ hg echoall 'test $2' foo
  test $2 foo
  $ hg echoall 'test $@' foo '$@'
  test $@ foo $@
  $ hg echoall 'test "$@"' foo '"$@"'
  test "$@" foo "$@"
  $ hg echo1 foo bar baz
  foo
  $ hg echo2 foo bar baz
  bar
  $ hg echo13 foo bar baz test
  foo baz
  $ hg echo2 foo
  

  $ hg echotokens
  

  $ hg echotokens foo 'bar $1 baz'
  foo
  bar $1 baz
  $ hg echotokens 'test $2' foo
  test $2
  foo
  $ hg echotokens 'test $@' foo '$@'
  test $@
  foo
  $@
  $ hg echotokens 'test "$@"' foo '"$@"'
  test "$@"
  foo
  "$@"
  $ echo bar > bar
  $ hg commit -qA -m bar
  $ hg count .
  1
  $ hg count 'branch(default)'
  2
  $ hg mcount -r '"branch(default)"'
  2

  $ tglog
  @  1: c0c7cf58edc5 'bar'
  |
  o  0: e63c23eaa88a 'foo'
  



shadowing

  $ hg i
  hg: command 'i' is ambiguous:
  	id or identify
  	idalias
  	idaliaslong
  	idaliasshell
  	import
  	in or incoming
  	init
  [255]
  $ hg id
  c0c7cf58edc5 tip
  $ hg ida
  hg: command 'ida' is ambiguous:
  	idalias
  	idaliaslong
  	idaliasshell
  [255]
  $ hg idalias
  c0c7cf58edc5 tip
  $ hg idaliasl
  c0c7cf58edc5 tip
  $ hg idaliass
  test
  $ hg parentsshell
  hg: command 'parentsshell' is ambiguous:
  	parentsshell1
  	parentsshell2
  [255]
  $ hg parentsshell1
  one
  $ hg parentsshell2
  two


shell aliases with global options

  $ hg init sub
  $ cd sub
  $ hg count 'branch(default)'
  0
  $ hg -v count 'branch(default)'
  0
  $ hg -R .. count 'branch(default)'
  warning: --repository ignored
  0
  $ hg --cwd .. count 'branch(default)'
  2

global flags after the shell alias name is passed to the shell command, not handled by hg

  $ hg echoall --cwd ..
  abort: option --cwd may not be abbreviated!
  [255]


"--" passed to shell alias should be preserved

  $ hg --config alias.printf='!printf "$@"' printf '%s %s %s\n' -- --cwd ..
  -- --cwd ..

repo specific shell aliases

  $ cat >> .hg/hgrc <<EOF
  > [alias]
  > subalias = !echo sub
  > EOF
  $ cat >> ../.hg/hgrc <<EOF
  > [alias]
  > mainalias = !echo main
  > EOF


shell alias defined in current repo

  $ hg subalias
  sub
  $ hg --cwd .. subalias > /dev/null
  unknown command 'subalias'
  (use 'hg help' to get help)
  [255]
  $ hg -R .. subalias > /dev/null
  unknown command 'subalias'
  (use 'hg help' to get help)
  [255]


shell alias defined in other repo

  $ hg mainalias > /dev/null
  unknown command 'mainalias'
  (use 'hg help' to get help)
  [255]
  $ hg -R .. mainalias
  warning: --repository ignored
  main
  $ hg --cwd .. mainalias
  main

typos get useful suggestions
  $ hg --cwd .. manalias
  unknown command 'manalias'
  (use 'hg help' to get help)
  [255]

shell aliases with escaped $ chars

  $ hg escaped1
  test$test
  $ hg escaped2
  HGFOO is BAR
  $ hg escaped3 HGFOO
  HGFOO is BAR
  $ hg escaped4 test
  $0 $@

abbreviated name, which matches against both shell alias and the
command provided extension, should be aborted.

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > rebase =
  > EOF
  $ cat >> .hg/hgrc <<'EOF'
  > [alias]
  > rebate = !echo this is rebate $@
  > EOF
  $ hg reba
  hg: command 'reba' is ambiguous:
  	rebase
  	rebate
  [255]
  $ hg rebat
  this is rebate
  $ hg rebat --foo-bar
  this is rebate --foo-bar

invalid arguments

  $ hg rt foo
  hg root: invalid arguments
  (use 'hg root -h' to get help)
  [255]

invalid global arguments for normal commands, aliases, and shell aliases

  $ hg --invalid root
  unknown command '--invalid'
  (use 'hg help' to get help)
  [255]
  $ hg --invalid mylog
  unknown command '--invalid'
  (use 'hg help' to get help)
  [255]
  $ hg --invalid blank
  unknown command '--invalid'
  (use 'hg help' to get help)
  [255]

environment variable changes in alias commands

  $ cat > $TESTTMP/expandalias.py <<EOF
  > import os
  > from edenscm.mercurial import cmdutil, commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('expandalias')
  > def expandalias(ui, repo, name):
  >     alias = cmdutil.findcmd(name, commands.table)[1][0]
  >     ui.write('%s args: %s\n' % (name, ' '.join(alias.args)))
  >     os.environ['COUNT'] = '2'
  >     ui.write('%s args: %s (with COUNT=2)\n' % (name, ' '.join(alias.args)))
  > EOF

  $ cat >> $HGRCPATH <<'EOF'
  > [extensions]
  > expandalias = $TESTTMP/expandalias.py
  > [alias]
  > showcount = log -T "$COUNT" -r .
  > EOF

  $ COUNT=1 hg expandalias showcount
  showcount args: -T 1 -r .
  showcount args: -T 2 -r . (with COUNT=2)

This should show id:

  $ hg --config alias.log='id' log
  000000000000 tip

This shouldn't:

  $ hg --config alias.log='id' history

  $ cd ../..

return code of command and shell aliases:

  $ hg mycommit -R alias
  nothing changed
  [1]
  $ hg exit1
  [1]

#if no-outer-repo
  $ hg root
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]
  $ hg --config alias.hgroot='!hg root' hgroot
  abort: no repository found in '$TESTTMP' (.hg not found)!
  [255]
#endif

documented aliases

  $ hg help documented
  [^ ].* (re) (?)
  
  an alias for the id command
  
  hg identify [-nibtB] [-r REV] [SOURCE]
  
  aliases: id
  
  identify the working directory or specified revision
  
      Print a summary identifying the repository state at REV using one or two
      parent hash identifiers, followed by a "+" if the working directory has
      uncommitted changes, a list of tags, and a list of bookmarks.
  
      When REV is not given, print a summary of the current state of the
      repository.
  
      Specifying a path to a repository root or Mercurial bundle will cause
      lookup to operate on that repository/bundle.
  
      See 'hg log' for generating more information about specific revisions,
      including full hash identifiers.
  
      Returns 0 if successful.
  
  Options:
  
   -r --rev REV       identify the specified revision
   -n --num           show local revision number
   -i --id            show global revision id
   -t --tags          show tags
   -B --bookmarks     show bookmarks
   -e --ssh CMD       specify ssh command to use
      --remotecmd CMD specify hg command to run on the remote side
  
  (some details hidden, use --verbose to show complete help)












  $ hg help commands | grep documented
   documented    an alias for the id command
