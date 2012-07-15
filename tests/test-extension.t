Test basic extension support

  $ cat > foobar.py <<EOF
  > import os
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > def uisetup(ui):
  >     ui.write("uisetup called\\n")
  > def reposetup(ui, repo):
  >     ui.write("reposetup called for %s\\n" % os.path.basename(repo.root))
  >     ui.write("ui %s= repo.ui\\n" % (ui == repo.ui and "=" or "!"))
  > @command('foo', [], 'hg foo')
  > def foo(ui, *args, **kwargs):
  >     ui.write("Foo\\n")
  > @command('bar', [], 'hg bar', norepo=True)
  > def bar(ui, *args, **kwargs):
  >     ui.write("Bar\\n")
  > EOF
  $ abspath=`pwd`/foobar.py

  $ mkdir barfoo
  $ cp foobar.py barfoo/__init__.py
  $ barfoopath=`pwd`/barfoo

  $ hg init a
  $ cd a
  $ echo foo > file
  $ hg add file
  $ hg commit -m 'add file'

  $ echo '[extensions]' >> $HGRCPATH
  $ echo "foobar = $abspath" >> $HGRCPATH
  $ hg foo
  uisetup called
  reposetup called for a
  ui == repo.ui
  Foo

  $ cd ..
  $ hg clone a b
  uisetup called
  reposetup called for a
  ui == repo.ui
  reposetup called for b
  ui == repo.ui
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg bar
  uisetup called
  Bar
  $ echo 'foobar = !' >> $HGRCPATH

module/__init__.py-style

  $ echo "barfoo = $barfoopath" >> $HGRCPATH
  $ cd a
  $ hg foo
  uisetup called
  reposetup called for a
  ui == repo.ui
  Foo
  $ echo 'barfoo = !' >> $HGRCPATH

Check that extensions are loaded in phases:

  $ cat > foo.py <<EOF
  > import os
  > name = os.path.basename(__file__).rsplit('.', 1)[0]
  > print "1) %s imported" % name
  > def uisetup(ui):
  >     print "2) %s uisetup" % name
  > def extsetup():
  >     print "3) %s extsetup" % name
  > def reposetup(ui, repo):
  >    print "4) %s reposetup" % name
  > EOF

  $ cp foo.py bar.py
  $ echo 'foo = foo.py' >> $HGRCPATH
  $ echo 'bar = bar.py' >> $HGRCPATH

Command with no output, we just want to see the extensions loaded:

  $ hg paths
  1) foo imported
  1) bar imported
  2) foo uisetup
  2) bar uisetup
  3) foo extsetup
  3) bar extsetup
  4) foo reposetup
  4) bar reposetup

Check hgweb's load order:

  $ cat > hgweb.cgi <<EOF
  > #!/usr/bin/env python
  > from mercurial import demandimport; demandimport.enable()
  > from mercurial.hgweb import hgweb
  > from mercurial.hgweb import wsgicgi
  > application = hgweb('.', 'test repo')
  > wsgicgi.launch(application)
  > EOF

  $ REQUEST_METHOD='GET' PATH_INFO='/' SCRIPT_NAME='' QUERY_STRING='' \
  >    SERVER_PORT='80' SERVER_NAME='localhost' python hgweb.cgi \
  >    | grep '^[0-9]) ' # ignores HTML output
  1) foo imported
  1) bar imported
  2) foo uisetup
  2) bar uisetup
  3) foo extsetup
  3) bar extsetup
  4) foo reposetup
  4) bar reposetup
  4) foo reposetup
  4) bar reposetup

  $ echo 'foo = !' >> $HGRCPATH
  $ echo 'bar = !' >> $HGRCPATH

Check "from __future__ import absolute_import" support for external libraries

#if windows
  $ PATHSEP=";"
#else
  $ PATHSEP=":"
#endif
  $ export PATHSEP

  $ mkdir $TESTTMP/libroot
  $ echo "s = 'libroot/ambig.py'" > $TESTTMP/libroot/ambig.py
  $ mkdir $TESTTMP/libroot/mod
  $ touch $TESTTMP/libroot/mod/__init__.py
  $ echo "s = 'libroot/mod/ambig.py'" > $TESTTMP/libroot/mod/ambig.py

#if absimport
  $ cat > $TESTTMP/libroot/mod/ambigabs.py <<EOF
  > from __future__ import absolute_import
  > import ambig # should load "libroot/ambig.py"
  > s = ambig.s
  > EOF
  $ cat > loadabs.py <<EOF
  > import mod.ambigabs as ambigabs
  > def extsetup():
  >     print 'ambigabs.s=%s' % ambigabs.s
  > EOF
  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}/libroot; hg --config extensions.loadabs=loadabs.py root)
  ambigabs.s=libroot/ambig.py
  $TESTTMP/a (glob)
#endif

#if no-py3k
  $ cat > $TESTTMP/libroot/mod/ambigrel.py <<EOF
  > import ambig # should load "libroot/mod/ambig.py"
  > s = ambig.s
  > EOF
  $ cat > loadrel.py <<EOF
  > import mod.ambigrel as ambigrel
  > def extsetup():
  >     print 'ambigrel.s=%s' % ambigrel.s
  > EOF
  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}/libroot; hg --config extensions.loadrel=loadrel.py root)
  ambigrel.s=libroot/mod/ambig.py
  $TESTTMP/a (glob)
#endif

Check absolute/relative import of extension specific modules

  $ mkdir $TESTTMP/extroot
  $ cat > $TESTTMP/extroot/bar.py <<EOF
  > s = 'this is extroot.bar'
  > EOF
  $ mkdir $TESTTMP/extroot/sub1
  $ cat > $TESTTMP/extroot/sub1/__init__.py <<EOF
  > s = 'this is extroot.sub1.__init__'
  > EOF
  $ cat > $TESTTMP/extroot/sub1/baz.py <<EOF
  > s = 'this is extroot.sub1.baz'
  > EOF
  $ cat > $TESTTMP/extroot/__init__.py <<EOF
  > s = 'this is extroot.__init__'
  > import foo
  > def extsetup(ui):
  >     ui.write('(extroot) ', foo.func(), '\n')
  > EOF

  $ cat > $TESTTMP/extroot/foo.py <<EOF
  > # test absolute import
  > buf = []
  > def func():
  >     # "not locals" case
  >     import extroot.bar
  >     buf.append('import extroot.bar in func(): %s' % extroot.bar.s)
  >     return '\n(extroot) '.join(buf)
  > # "fromlist == ('*',)" case
  > from extroot.bar import *
  > buf.append('from extroot.bar import *: %s' % s)
  > # "not fromlist" and "if '.' in name" case
  > import extroot.sub1.baz
  > buf.append('import extroot.sub1.baz: %s' % extroot.sub1.baz.s)
  > # "not fromlist" and NOT "if '.' in name" case
  > import extroot
  > buf.append('import extroot: %s' % extroot.s)
  > # NOT "not fromlist" and NOT "level != -1" case
  > from extroot.bar import s
  > buf.append('from extroot.bar import s: %s' % s)
  > EOF
  $ hg --config extensions.extroot=$TESTTMP/extroot root
  (extroot) from extroot.bar import *: this is extroot.bar
  (extroot) import extroot.sub1.baz: this is extroot.sub1.baz
  (extroot) import extroot: this is extroot.__init__
  (extroot) from extroot.bar import s: this is extroot.bar
  (extroot) import extroot.bar in func(): this is extroot.bar
  $TESTTMP/a (glob)

#if no-py3k
  $ rm "$TESTTMP"/extroot/foo.*
  $ cat > $TESTTMP/extroot/foo.py <<EOF
  > # test relative import
  > buf = []
  > def func():
  >     # "not locals" case
  >     import bar
  >     buf.append('import bar in func(): %s' % bar.s)
  >     return '\n(extroot) '.join(buf)
  > # "fromlist == ('*',)" case
  > from bar import *
  > buf.append('from bar import *: %s' % s)
  > # "not fromlist" and "if '.' in name" case
  > import sub1.baz
  > buf.append('import sub1.baz: %s' % sub1.baz.s)
  > # "not fromlist" and NOT "if '.' in name" case
  > import sub1
  > buf.append('import sub1: %s' % sub1.s)
  > # NOT "not fromlist" and NOT "level != -1" case
  > from bar import s
  > buf.append('from bar import s: %s' % s)
  > EOF
  $ hg --config extensions.extroot=$TESTTMP/extroot root
  (extroot) from bar import *: this is extroot.bar
  (extroot) import sub1.baz: this is extroot.sub1.baz
  (extroot) import sub1: this is extroot.sub1.__init__
  (extroot) from bar import s: this is extroot.bar
  (extroot) import bar in func(): this is extroot.bar
  $TESTTMP/a (glob)
#endif

  $ cd ..

hide outer repo
  $ hg init

  $ cat > empty.py <<EOF
  > '''empty cmdtable
  > '''
  > cmdtable = {}
  > EOF
  $ emptypath=`pwd`/empty.py
  $ echo "empty = $emptypath" >> $HGRCPATH
  $ hg help empty
  empty extension - empty cmdtable
  
  no commands defined


  $ echo 'empty = !' >> $HGRCPATH

  $ cat > debugextension.py <<EOF
  > '''only debugcommands
  > '''
  > from mercurial import cmdutil
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('debugfoobar', [], 'hg debugfoobar')
  > def debugfoobar(ui, repo, *args, **opts):
  >     "yet another debug command"
  >     pass
  > @command('foo', [], 'hg foo')
  > def foo(ui, repo, *args, **opts):
  >     """yet another foo command
  >     This command has been DEPRECATED since forever.
  >     """
  >     pass
  > EOF
  $ debugpath=`pwd`/debugextension.py
  $ echo "debugextension = $debugpath" >> $HGRCPATH

  $ hg help debugextension
  debugextension extension - only debugcommands
  
  no commands defined


  $ hg --verbose help debugextension
  debugextension extension - only debugcommands
  
  list of commands:
  
   foo           yet another foo command
  
  global options ([+] can be repeated):
  
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






  $ hg --debug help debugextension
  debugextension extension - only debugcommands
  
  list of commands:
  
   debugfoobar   yet another debug command
   foo           yet another foo command
  
  global options ([+] can be repeated):
  
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





  $ echo 'debugextension = !' >> $HGRCPATH

Extension module help vs command help:

  $ echo 'extdiff =' >> $HGRCPATH
  $ hg help extdiff
  hg extdiff [OPT]... [FILE]...
  
  use external program to diff repository (or selected files)
  
      Show differences between revisions for the specified files, using an
      external program. The default program used is diff, with default options
      "-Npru".
  
      To select a different program, use the -p/--program option. The program
      will be passed the names of two directories to compare. To pass additional
      options to the program, use -o/--option. These will be passed before the
      names of the directories to compare.
  
      When two revision arguments are given, then changes are shown between
      those revisions. If only one revision is specified then that revision is
      compared to the working directory, and, when no revisions are specified,
      the working directory files are compared to its parent.
  
  (use "hg help -e extdiff" to show help for the extdiff extension)
  
  options ([+] can be repeated):
  
   -p --program CMD         comparison program to run
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
   -S --subrepos            recurse into subrepositories
  
  (some details hidden, use --verbose to show complete help)










  $ hg help --extension extdiff
  extdiff extension - command to allow external programs to compare revisions
  
  The extdiff Mercurial extension allows you to use external programs to compare
  revisions, or revision with working directory. The external diff programs are
  called with a configurable set of options and two non-option arguments: paths
  to directories containing snapshots of files to compare.
  
  The extdiff extension also allows you to configure new diff commands, so you
  do not need to type "hg extdiff -p kdiff3" always.
  
    [extdiff]
    # add new command that runs GNU diff(1) in 'context diff' mode
    cdiff = gdiff -Nprc5
    ## or the old way:
    #cmd.cdiff = gdiff
    #opts.cdiff = -Nprc5
  
    # add new command called meld, runs meld (no need to name twice).  If
    # the meld executable is not available, the meld tool in [merge-tools]
    # will be used, if available
    meld =
  
    # add new command called vimdiff, runs gvimdiff with DirDiff plugin
    # (see http://www.vim.org/scripts/script.php?script_id=102) Non
    # English user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
    # your .vimrc
    vimdiff = gvim -f "+next" \
              "+execute 'DirDiff' fnameescape(argv(0)) fnameescape(argv(1))"
  
  Tool arguments can include variables that are expanded at runtime:
  
    $parent1, $plabel1 - filename, descriptive label of first parent
    $child,   $clabel  - filename, descriptive label of child revision
    $parent2, $plabel2 - filename, descriptive label of second parent
    $root              - repository root
    $parent is an alias for $parent1.
  
  The extdiff extension will look in your [diff-tools] and [merge-tools]
  sections for diff tool arguments, when none are specified in [extdiff].
  
    [extdiff]
    kdiff3 =
  
    [diff-tools]
    kdiff3.diffargs=--L1 '$plabel1' --L2 '$clabel' $parent $child
  
  You can use -I/-X and list of file or directory names like normal "hg diff"
  command. The extdiff extension makes snapshots of only needed files, so
  running the external diff program will actually be pretty fast (at least
  faster than having to compare the entire tree).
  
  list of commands:
  
   extdiff       use external program to diff repository (or selected files)
  
  (use "hg help -v -e extdiff" to show built-in aliases and global options)
















  $ echo 'extdiff = !' >> $HGRCPATH

Test help topic with same name as extension

  $ cat > multirevs.py <<EOF
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > """multirevs extension
  > Big multi-line module docstring."""
  > @command('multirevs', [], 'ARG', norepo=True)
  > def multirevs(ui, repo, arg, *args, **opts):
  >     """multirevs command"""
  >     pass
  > EOF
  $ echo "multirevs = multirevs.py" >> $HGRCPATH

  $ hg help multirevs
  Specifying Multiple Revisions
  """""""""""""""""""""""""""""
  
      When Mercurial accepts more than one revision, they may be specified
      individually, or provided as a topologically continuous range, separated
      by the ":" character.
  
      The syntax of range notation is [BEGIN]:[END], where BEGIN and END are
      revision identifiers. Both BEGIN and END are optional. If BEGIN is not
      specified, it defaults to revision number 0. If END is not specified, it
      defaults to the tip. The range ":" thus means "all revisions".
  
      If BEGIN is greater than END, revisions are treated in reverse order.
  
      A range acts as a closed interval. This means that a range of 3:5 gives 3,
      4 and 5. Similarly, a range of 9:6 gives 9, 8, 7, and 6.
  
  use "hg help -c multirevs" to see help for the multirevs command






  $ hg help -c multirevs
  hg multirevs ARG
  
  multirevs command
  
  (some details hidden, use --verbose to show complete help)



  $ hg multirevs
  hg multirevs: invalid arguments
  hg multirevs ARG
  
  multirevs command
  
  (use "hg multirevs -h" to show more help)
  [255]



  $ echo "multirevs = !" >> $HGRCPATH

Issue811: Problem loading extensions twice (by site and by user)

  $ debugpath=`pwd`/debugissue811.py
  $ cat > debugissue811.py <<EOF
  > '''show all loaded extensions
  > '''
  > from mercurial import cmdutil, commands, extensions
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('debugextensions', [], 'hg debugextensions', norepo=True)
  > def debugextensions(ui):
  >     "yet another debug command"
  >     ui.write("%s\n" % '\n'.join([x for x, y in extensions.extensions()]))
  > EOF
  $ cat <<EOF >> $HGRCPATH
  > debugissue811 = $debugpath
  > mq =
  > strip =
  > hgext.mq =
  > hgext/mq =
  > EOF

Show extensions:
(note that mq force load strip, also checking it's not loaded twice)

  $ hg debugextensions
  debugissue811
  strip
  mq

For extensions, which name matches one of its commands, help
message should ask '-v -e' to get list of built-in aliases
along with extension help itself

  $ mkdir $TESTTMP/d
  $ cat > $TESTTMP/d/dodo.py <<EOF
  > """
  > This is an awesome 'dodo' extension. It does nothing and
  > writes 'Foo foo'
  > """
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('dodo', [], 'hg dodo')
  > def dodo(ui, *args, **kwargs):
  >     """Does nothing"""
  >     ui.write("I do nothing. Yay\\n")
  > @command('foofoo', [], 'hg foofoo')
  > def foofoo(ui, *args, **kwargs):
  >     """Writes 'Foo foo'"""
  >     ui.write("Foo foo\\n")
  > EOF
  $ dodopath=$TESTTMP/d/dodo.py

  $ echo "dodo = $dodopath" >> $HGRCPATH

Make sure that user is asked to enter '-v -e' to get list of built-in aliases
  $ hg help -e dodo
  dodo extension -
  
  This is an awesome 'dodo' extension. It does nothing and writes 'Foo foo'
  
  list of commands:
  
   dodo          Does nothing
   foofoo        Writes 'Foo foo'
  
  (use "hg help -v -e dodo" to show built-in aliases and global options)

Make sure that '-v -e' prints list of built-in aliases along with
extension help itself
  $ hg help -v -e dodo
  dodo extension -
  
  This is an awesome 'dodo' extension. It does nothing and writes 'Foo foo'
  
  list of commands:
  
   dodo          Does nothing
   foofoo        Writes 'Foo foo'
  
  global options ([+] can be repeated):
  
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

Make sure that single '-v' option shows help and built-ins only for 'dodo' command
  $ hg help -v dodo
  hg dodo
  
  Does nothing
  
  (use "hg help -e dodo" to show help for the dodo extension)
  
  options:
  
    --mq operate on patch repository
  
  global options ([+] can be repeated):
  
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

In case when extension name doesn't match any of its commands,
help message should ask for '-v' to get list of built-in aliases
along with extension help
  $ cat > $TESTTMP/d/dudu.py <<EOF
  > """
  > This is an awesome 'dudu' extension. It does something and
  > also writes 'Beep beep'
  > """
  > from mercurial import cmdutil, commands
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command('something', [], 'hg something')
  > def something(ui, *args, **kwargs):
  >     """Does something"""
  >     ui.write("I do something. Yaaay\\n")
  > @command('beep', [], 'hg beep')
  > def beep(ui, *args, **kwargs):
  >     """Writes 'Beep beep'"""
  >     ui.write("Beep beep\\n")
  > EOF
  $ dudupath=$TESTTMP/d/dudu.py

  $ echo "dudu = $dudupath" >> $HGRCPATH

  $ hg help -e dudu
  dudu extension -
  
  This is an awesome 'dudu' extension. It does something and also writes 'Beep
  beep'
  
  list of commands:
  
   beep          Writes 'Beep beep'
   something     Does something
  
  (use "hg help -v dudu" to show built-in aliases and global options)

In case when extension name doesn't match any of its commands,
help options '-v' and '-v -e' should be equivalent
  $ hg help -v dudu
  dudu extension -
  
  This is an awesome 'dudu' extension. It does something and also writes 'Beep
  beep'
  
  list of commands:
  
   beep          Writes 'Beep beep'
   something     Does something
  
  global options ([+] can be repeated):
  
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

  $ hg help -v -e dudu
  dudu extension -
  
  This is an awesome 'dudu' extension. It does something and also writes 'Beep
  beep'
  
  list of commands:
  
   beep          Writes 'Beep beep'
   something     Does something
  
  global options ([+] can be repeated):
  
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

Disabled extension commands:

  $ ORGHGRCPATH=$HGRCPATH
  $ HGRCPATH=
  $ export HGRCPATH
  $ hg help email
  'email' is provided by the following extension:
  
      patchbomb     command to send changesets as (a series of) patch emails
  
  (use "hg help extensions" for information on enabling extensions)


  $ hg qdel
  hg: unknown command 'qdel'
  'qdelete' is provided by the following extension:
  
      mq            manage a stack of patches
  
  (use "hg help extensions" for information on enabling extensions)
  [255]


  $ hg churn
  hg: unknown command 'churn'
  'churn' is provided by the following extension:
  
      churn         command to display statistics about repository history
  
  (use "hg help extensions" for information on enabling extensions)
  [255]



Disabled extensions:

  $ hg help churn
  churn extension - command to display statistics about repository history
  
  (use "hg help extensions" for information on enabling extensions)

  $ hg help patchbomb
  patchbomb extension - command to send changesets as (a series of) patch emails
  
  (use "hg help extensions" for information on enabling extensions)


Broken disabled extension and command:

  $ mkdir hgext
  $ echo > hgext/__init__.py
  $ cat > hgext/broken.py <<EOF
  > "broken extension'
  > EOF
  $ cat > path.py <<EOF
  > import os, sys
  > sys.path.insert(0, os.environ['HGEXTPATH'])
  > EOF
  $ HGEXTPATH=`pwd`
  $ export HGEXTPATH

  $ hg --config extensions.path=./path.py help broken
  broken extension - (no help text available)
  
  (use "hg help extensions" for information on enabling extensions)


  $ cat > hgext/forest.py <<EOF
  > cmdtable = None
  > EOF
  $ hg --config extensions.path=./path.py help foo > /dev/null
  warning: error finding commands in $TESTTMP/hgext/forest.py (glob)
  abort: no such help topic: foo
  (try "hg help --keyword foo")
  [255]

  $ cat > throw.py <<EOF
  > from mercurial import cmdutil, commands, util
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > class Bogon(Exception): pass
  > @command('throw', [], 'hg throw', norepo=True)
  > def throw(ui, **opts):
  >     """throws an exception"""
  >     raise Bogon()
  > EOF

No declared supported version, extension complains:
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension throw
  ** which supports versions unknown of Mercurial.
  ** Please disable throw and try your action again.
  ** If that fixes the bug please report it to the extension author.
  ** Python * (glob)
  ** Mercurial Distributed SCM * (glob)
  ** Extensions loaded: throw

empty declaration of supported version, extension complains:
  $ echo "testedwith = ''" >> throw.py
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension throw
  ** which supports versions unknown of Mercurial.
  ** Please disable throw and try your action again.
  ** If that fixes the bug please report it to the extension author.
  ** Python * (glob)
  ** Mercurial Distributed SCM (*) (glob)
  ** Extensions loaded: throw

If the extension specifies a buglink, show that:
  $ echo 'buglink = "http://example.com/bts"' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension throw
  ** which supports versions unknown of Mercurial.
  ** Please disable throw and try your action again.
  ** If that fixes the bug please report it to http://example.com/bts
  ** Python * (glob)
  ** Mercurial Distributed SCM (*) (glob)
  ** Extensions loaded: throw

If the extensions declare outdated versions, accuse the older extension first:
  $ echo "from mercurial import util" >> older.py
  $ echo "util.version = lambda:'2.2'" >> older.py
  $ echo "testedwith = '1.9.3'" >> older.py
  $ echo "testedwith = '2.1.1'" >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension older
  ** which supports versions 1.9 of Mercurial.
  ** Please disable older and try your action again.
  ** If that fixes the bug please report it to the extension author.
  ** Python * (glob)
  ** Mercurial Distributed SCM (version 2.2)
  ** Extensions loaded: throw, older

One extension only tested with older, one only with newer versions:
  $ echo "util.version = lambda:'2.1'" >> older.py
  $ rm -f older.pyc older.pyo
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension older
  ** which supports versions 1.9 of Mercurial.
  ** Please disable older and try your action again.
  ** If that fixes the bug please report it to the extension author.
  ** Python * (glob)
  ** Mercurial Distributed SCM (version 2.1)
  ** Extensions loaded: throw, older

Older extension is tested with current version, the other only with newer:
  $ echo "util.version = lambda:'1.9.3'" >> older.py
  $ rm -f older.pyc older.pyo
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*'
  ** Unknown exception encountered with possibly-broken third-party extension throw
  ** which supports versions 2.1 of Mercurial.
  ** Please disable throw and try your action again.
  ** If that fixes the bug please report it to http://example.com/bts
  ** Python * (glob)
  ** Mercurial Distributed SCM (version 1.9.3)
  ** Extensions loaded: throw, older

Declare the version as supporting this hg version, show regular bts link:
  $ hgver=`$PYTHON -c 'from mercurial import util; print util.version().split("+")[0]'`
  $ echo 'testedwith = """'"$hgver"'"""' >> throw.py
  $ if [ -z "$hgver" ]; then
  >   echo "unable to fetch a mercurial version. Make sure __version__ is correct";
  > fi
  $ rm -f throw.pyc throw.pyo
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** unknown exception encountered, please report by visiting
  ** http://mercurial.selenic.com/wiki/BugTracker
  ** Python * (glob)
  ** Mercurial Distributed SCM (*) (glob)
  ** Extensions loaded: throw

Patch version is ignored during compatibility check
  $ echo "testedwith = '3.2'" >> throw.py
  $ echo "util.version = lambda:'3.2.2'" >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** unknown exception encountered, please report by visiting
  ** http://mercurial.selenic.com/wiki/BugTracker
  ** Python * (glob)
  ** Mercurial Distributed SCM (*) (glob)
  ** Extensions loaded: throw

Test version number support in 'hg version':
  $ echo '__version__ = (1, 2, 3)' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ hg version -v
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-* Matt Mackall and others (glob)
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  
  Enabled extensions:
  

  $ hg version -v --config extensions.throw=throw.py
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-* Matt Mackall and others (glob)
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  
  Enabled extensions:
  
    throw  1.2.3
  $ echo 'getversion = lambda: "1.twentythree"' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ hg version -v --config extensions.throw=throw.py
  Mercurial Distributed SCM (version *) (glob)
  (see http://mercurial.selenic.com for more information)
  
  Copyright (C) 2005-* Matt Mackall and others (glob)
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  
  Enabled extensions:
  
    throw  1.twentythree

Restore HGRCPATH

  $ HGRCPATH=$ORGHGRCPATH
  $ export HGRCPATH

Commands handling multiple repositories at a time should invoke only
"reposetup()" of extensions enabling in the target repository.

  $ mkdir reposetup-test
  $ cd reposetup-test

  $ cat > $TESTTMP/reposetuptest.py <<EOF
  > from mercurial import extensions
  > def reposetup(ui, repo):
  >     ui.write('reposetup() for %s\n' % (repo.root))
  > EOF
  $ hg init src
  $ echo a > src/a
  $ hg -R src commit -Am '#0 at src/a'
  adding a
  $ echo '[extensions]' >> src/.hg/hgrc
  $ echo '# enable extension locally' >> src/.hg/hgrc
  $ echo "reposetuptest = $TESTTMP/reposetuptest.py" >> src/.hg/hgrc
  $ hg -R src status
  reposetup() for $TESTTMP/reposetup-test/src (glob)

  $ hg clone -U src clone-dst1
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  $ hg init push-dst1
  $ hg -q -R src push push-dst1
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  $ hg init pull-src1
  $ hg -q -R pull-src1 pull src
  reposetup() for $TESTTMP/reposetup-test/src (glob)

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > # disable extension globally and explicitly
  > reposetuptest = !
  > EOF
  $ hg clone -U src clone-dst2
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  $ hg init push-dst2
  $ hg -q -R src push push-dst2
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  $ hg init pull-src2
  $ hg -q -R pull-src2 pull src
  reposetup() for $TESTTMP/reposetup-test/src (glob)

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > # enable extension globally
  > reposetuptest = $TESTTMP/reposetuptest.py
  > EOF
  $ hg clone -U src clone-dst3
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  reposetup() for $TESTTMP/reposetup-test/clone-dst3 (glob)
  $ hg init push-dst3
  reposetup() for $TESTTMP/reposetup-test/push-dst3 (glob)
  $ hg -q -R src push push-dst3
  reposetup() for $TESTTMP/reposetup-test/src (glob)
  reposetup() for $TESTTMP/reposetup-test/push-dst3 (glob)
  $ hg init pull-src3
  reposetup() for $TESTTMP/reposetup-test/pull-src3 (glob)
  $ hg -q -R pull-src3 pull src
  reposetup() for $TESTTMP/reposetup-test/pull-src3 (glob)
  reposetup() for $TESTTMP/reposetup-test/src (glob)

  $ echo '[extensions]' >> src/.hg/hgrc
  $ echo '# disable extension locally' >> src/.hg/hgrc
  $ echo 'reposetuptest = !' >> src/.hg/hgrc
  $ hg clone -U src clone-dst4
  reposetup() for $TESTTMP/reposetup-test/clone-dst4 (glob)
  $ hg init push-dst4
  reposetup() for $TESTTMP/reposetup-test/push-dst4 (glob)
  $ hg -q -R src push push-dst4
  reposetup() for $TESTTMP/reposetup-test/push-dst4 (glob)
  $ hg init pull-src4
  reposetup() for $TESTTMP/reposetup-test/pull-src4 (glob)
  $ hg -q -R pull-src4 pull src
  reposetup() for $TESTTMP/reposetup-test/pull-src4 (glob)

disabling in command line overlays with all configuration
  $ hg --config extensions.reposetuptest=! clone -U src clone-dst5
  $ hg --config extensions.reposetuptest=! init push-dst5
  $ hg --config extensions.reposetuptest=! -q -R src push push-dst5
  $ hg --config extensions.reposetuptest=! init pull-src5
  $ hg --config extensions.reposetuptest=! -q -R pull-src5 pull src

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > # disable extension globally and explicitly
  > reposetuptest = !
  > EOF
  $ hg init parent
  $ hg init parent/sub1
  $ echo 1 > parent/sub1/1
  $ hg -R parent/sub1 commit -Am '#0 at parent/sub1'
  adding 1
  $ hg init parent/sub2
  $ hg init parent/sub2/sub21
  $ echo 21 > parent/sub2/sub21/21
  $ hg -R parent/sub2/sub21 commit -Am '#0 at parent/sub2/sub21'
  adding 21
  $ cat > parent/sub2/.hgsub <<EOF
  > sub21 = sub21
  > EOF
  $ hg -R parent/sub2 commit -Am '#0 at parent/sub2'
  adding .hgsub
  $ hg init parent/sub3
  $ echo 3 > parent/sub3/3
  $ hg -R parent/sub3 commit -Am '#0 at parent/sub3'
  adding 3
  $ cat > parent/.hgsub <<EOF
  > sub1 = sub1
  > sub2 = sub2
  > sub3 = sub3
  > EOF
  $ hg -R parent commit -Am '#0 at parent'
  adding .hgsub
  $ echo '[extensions]' >> parent/.hg/hgrc
  $ echo '# enable extension locally' >> parent/.hg/hgrc
  $ echo "reposetuptest = $TESTTMP/reposetuptest.py" >> parent/.hg/hgrc
  $ cp parent/.hg/hgrc parent/sub2/.hg/hgrc
  $ hg -R parent status -S -A
  reposetup() for $TESTTMP/reposetup-test/parent (glob)
  reposetup() for $TESTTMP/reposetup-test/parent/sub2 (glob)
  C .hgsub
  C .hgsubstate
  C sub1/1
  C sub2/.hgsub
  C sub2/.hgsubstate
  C sub2/sub21/21
  C sub3/3

  $ cd ..

Test synopsis and docstring extending

  $ hg init exthelp
  $ cat > exthelp.py <<EOF
  > from mercurial import commands, extensions
  > def exbookmarks(orig, *args, **opts):
  >     return orig(*args, **opts)
  > def uisetup(ui):
  >     synopsis = ' GREPME [--foo] [-x]'
  >     docstring = '''
  >     GREPME make sure that this is in the help!
  >     '''
  >     extensions.wrapcommand(commands.table, 'bookmarks', exbookmarks,
  >                            synopsis, docstring)
  > EOF
  $ abspath=`pwd`/exthelp.py
  $ echo '[extensions]' >> $HGRCPATH
  $ echo "exthelp = $abspath" >> $HGRCPATH
  $ cd exthelp
  $ hg help bookmarks | grep GREPME
  hg bookmarks [OPTIONS]... [NAME]... GREPME [--foo] [-x]
      GREPME make sure that this is in the help!

