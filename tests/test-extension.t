Test basic extension support

  $ "$TESTDIR/hghave" no-outer-repo || exit 80

  $ cat > foobar.py <<EOF
  > import os
  > from mercurial import commands
  > 
  > def uisetup(ui):
  >     ui.write("uisetup called\\n")
  > 
  > def reposetup(ui, repo):
  >     ui.write("reposetup called for %s\\n" % os.path.basename(repo.root))
  >     ui.write("ui %s= repo.ui\\n" % (ui == repo.ui and "=" or "!"))
  > 
  > def foo(ui, *args, **kwargs):
  >     ui.write("Foo\\n")
  > 
  > def bar(ui, *args, **kwargs):
  >     ui.write("Bar\\n")
  > 
  > cmdtable = {
  >    "foo": (foo, [], "hg foo"),
  >    "bar": (bar, [], "hg bar"),
  > }
  > 
  > commands.norepo += ' bar'
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
  > 
  > application = hgweb('.', 'test repo')
  > wsgicgi.launch(application)
  > EOF

  $ SCRIPT_NAME='/' SERVER_PORT='80' SERVER_NAME='localhost' python hgweb.cgi \
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

  $ cd ..

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
  > def debugfoobar(ui, repo, *args, **opts):
  >     "yet another debug command"
  >     pass
  > 
  > def foo(ui, repo, *args, **opts):
  >     """yet another foo command
  > 
  >     This command has been DEPRECATED since forever.
  >     """
  >     pass
  > 
  > cmdtable = {
  >    "debugfoobar": (debugfoobar, (), "hg debugfoobar"),
  >    "foo": (foo, (), "hg foo")
  > }
  > EOF
  $ debugpath=`pwd`/debugextension.py
  $ echo "debugextension = $debugpath" >> $HGRCPATH

  $ hg help debugextension
  debugextension extension - only debugcommands
  
  no commands defined

  $ hg --verbose help debugextension
  debugextension extension - only debugcommands
  
  list of commands:
  
   foo:
        yet another foo command
  
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

  $ hg --debug help debugextension
  debugextension extension - only debugcommands
  
  list of commands:
  
   debugfoobar:
        yet another debug command
   foo:
        yet another foo command
  
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
  
  use "hg help -e extdiff" to show help for the extdiff extension
  
  options:
  
   -p --program CMD         comparison program to run
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  [+] marked option can be specified multiple times
  
  use "hg -v help extdiff" to show more info

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
  
    # add new command called vdiff, runs kdiff3
    vdiff = kdiff3
  
    # add new command called meld, runs meld (no need to name twice)
    meld =
  
    # add new command called vimdiff, runs gvimdiff with DirDiff plugin
    # (see http://www.vim.org/scripts/script.php?script_id=102) Non
    # English user, be sure to put "let g:DirDiffDynamicDiffText = 1" in
    # your .vimrc
    vimdiff = gvim -f '+next' '+execute "DirDiff" argv(0) argv(1)'
  
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
  
   extdiff   use external program to diff repository (or selected files)
  
  use "hg -v help extdiff" to show builtin aliases and global options

  $ echo 'extdiff = !' >> $HGRCPATH

Test help topic with same name as extension

  $ cat > multirevs.py <<EOF
  > from mercurial import commands
  > """multirevs extension
  > Big multi-line module docstring."""
  > def multirevs(ui, repo, arg, *args, **opts):
  >     """multirevs command"""
  >     pass
  > cmdtable = {
  >    "multirevs": (multirevs, [], 'ARG')
  > }
  > commands.norepo += ' multirevs'
  > EOF
  $ echo "multirevs = multirevs.py" >> $HGRCPATH

  $ hg help multirevs
  Specifying Multiple Revisions
  
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
  
  use "hg -v help multirevs" to show more info

  $ hg multirevs
  hg multirevs: invalid arguments
  hg multirevs ARG
  
  multirevs command
  
  use "hg help multirevs" to show the full help text
  [255]

  $ echo "multirevs = !" >> $HGRCPATH

Issue811: Problem loading extensions twice (by site and by user)

  $ debugpath=`pwd`/debugissue811.py
  $ cat > debugissue811.py <<EOF
  > '''show all loaded extensions
  > '''
  > from mercurial import extensions, commands
  > 
  > def debugextensions(ui):
  >     "yet another debug command"
  >     ui.write("%s\n" % '\n'.join([x for x, y in extensions.extensions()]))
  > 
  > cmdtable = {"debugextensions": (debugextensions, (), "hg debugextensions")}
  > commands.norepo += " debugextensions"
  > EOF
  $ echo "debugissue811 = $debugpath" >> $HGRCPATH
  $ echo "mq=" >> $HGRCPATH
  $ echo "hgext.mq=" >> $HGRCPATH
  $ echo "hgext/mq=" >> $HGRCPATH

Show extensions:

  $ hg debugextensions
  debugissue811
  mq

Disabled extension commands:

  $ HGRCPATH=
  $ export HGRCPATH
  $ hg help email
  'email' is provided by the following extension:
  
      patchbomb  command to send changesets as (a series of) patch emails
  
  use "hg help extensions" for information on enabling extensions
  $ hg qdel
  hg: unknown command 'qdel'
  'qdelete' is provided by the following extension:
  
      mq  manage a stack of patches
  
  use "hg help extensions" for information on enabling extensions
  [255]
  $ hg churn
  hg: unknown command 'churn'
  'churn' is provided by the following extension:
  
      churn  command to display statistics about repository history
  
  use "hg help extensions" for information on enabling extensions
  [255]

Disabled extensions:

  $ hg help churn
  churn extension - command to display statistics about repository history
  
  use "hg help extensions" for information on enabling extensions
  $ hg help patchbomb
  patchbomb extension - command to send changesets as (a series of) patch emails
  
  use "hg help extensions" for information on enabling extensions

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
  
  use "hg help extensions" for information on enabling extensions

  $ cat > hgext/forest.py <<EOF
  > cmdtable = None
  > EOF
  $ hg --config extensions.path=./path.py help foo > /dev/null
  warning: error finding commands in $TESTTMP/hgext/forest.py
  hg: unknown command 'foo'
  warning: error finding commands in $TESTTMP/hgext/forest.py
  [255]
