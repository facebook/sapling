  $ setconfig extensions.treemanifest=!
#require no-fsmonitor

Test basic extension support

  $ cat > foobar.py <<EOF
  > import os
  > from edenscm.mercurial import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > configtable = {}
  > configitem = registrar.configitem(configtable)
  > configitem('tests', 'foo', default="Foo")
  > def uisetup(ui):
  >     ui.write("uisetup called\\n")
  >     ui.flush()
  > def reposetup(ui, repo):
  >     ui.write("reposetup called for %s\\n" % os.path.basename(repo.root))
  >     ui.write("ui %s= repo.ui\\n" % (ui == repo.ui and "=" or "!"))
  >     ui.flush()
  > @command(b'foo', [], 'hg foo')
  > def foo(ui, *args, **kwargs):
  >     foo = ui.config('tests', 'foo')
  >     ui.write(foo)
  >     ui.write("\\n")
  > @command(b'bar', [], 'hg bar', norepo=True)
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
  reposetup called for a (chg !)
  ui == repo.ui (chg !)
  Foo

  $ cd ..
  $ hg clone a b
  uisetup called (no-chg !)
  reposetup called for a
  ui == repo.ui
  reposetup called for b
  ui == repo.ui
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg bar
  uisetup called (no-chg !)
  Bar
  $ echo 'foobar = !' >> $HGRCPATH

module/__init__.py-style

  $ echo "barfoo = $barfoopath" >> $HGRCPATH
  $ cd a
  $ hg foo
  uisetup called
  reposetup called for a
  ui == repo.ui
  reposetup called for a (chg !)
  ui == repo.ui (chg !)
  Foo
  $ echo 'barfoo = !' >> $HGRCPATH

Check that extensions are loaded in phases:

  $ cat > foo.py <<EOF
  > import os
  > name = os.path.basename(__file__).rsplit('.', 1)[0]
  > print("1) %s imported" % name)
  > def uisetup(ui):
  >     print("2) %s uisetup" % name)
  > def extsetup():
  >     print("3) %s extsetup" % name)
  > def reposetup(ui, repo):
  >    print("4) %s reposetup" % name)
  > 
  > # custom predicate to check registration of functions at loading
  > from edenscm.mercurial import (
  >     registrar,
  >     smartset,
  > )
  > revsetpredicate = registrar.revsetpredicate()
  > @revsetpredicate(name, safe=True) # safe=True for query via hgweb
  > def custompredicate(repo, subset, x):
  >     return smartset.baseset([r for r in subset if r in {0}])
  > EOF

  $ cp foo.py bar.py
  $ echo 'foo = foo.py' >> $HGRCPATH
  $ echo 'bar = bar.py' >> $HGRCPATH

Check normal command's load order of extensions and registration of functions

  $ hg log -r "foo() and bar()" -q
  1) foo imported
  1) bar imported
  2) foo uisetup
  2) bar uisetup
  3) foo extsetup
  3) bar extsetup
  4) foo reposetup
  4) bar reposetup
  0:c24b9ac61126

Check hgweb's load order of extensions and registration of functions

  $ cat > hgweb.cgi <<EOF
  > #!$PYTHON
  > from edenscm.mercurial import demandimport; demandimport.enable()
  > from edenscm.mercurial.hgweb import hgweb
  > from edenscm.mercurial.hgweb import wsgicgi
  > application = hgweb('.', 'test repo')
  > wsgicgi.launch(application)
  > EOF
  $ . "$TESTDIR/cgienv"

  $ PATH_INFO='/' SCRIPT_NAME='' $PYTHON hgweb.cgi \
  >    | grep '^[0-9]) ' # ignores HTML output
  1) foo imported
  1) bar imported
  2) foo uisetup
  2) bar uisetup
  3) foo extsetup
  3) bar extsetup
  4) foo reposetup
  4) bar reposetup

(check that revset predicate foo() and bar() are available)

#if msys
  $ PATH_INFO='//shortlog'
#else
  $ PATH_INFO='/shortlog'
#endif
  $ export PATH_INFO
  $ SCRIPT_NAME='' QUERY_STRING='rev=foo() and bar()' $PYTHON hgweb.cgi \
  >     | grep '<a href="/rev/[0-9a-z]*">'
     <a href="/rev/c24b9ac61126">add file</a>

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

  $ cat > $TESTTMP/libroot/mod/ambigabs.py <<EOF
  > from __future__ import absolute_import
  > import ambig # should load "libroot/ambig.py"
  > s = ambig.s
  > EOF
  $ cat > loadabs.py <<EOF
  > import mod.ambigabs as ambigabs
  > def extsetup():
  >     print('ambigabs.s=%s' % ambigabs.s)
  > EOF
  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}/libroot; hg --config extensions.loadabs=loadabs.py log -r null -T "foo\n")
  ambigabs.s=libroot/ambig.py
  foo

#if no-py3k
  $ cat > $TESTTMP/libroot/mod/ambigrel.py <<EOF
  > import ambig # should load "libroot/mod/ambig.py"
  > s = ambig.s
  > EOF
  $ cat > loadrel.py <<EOF
  > import mod.ambigrel as ambigrel
  > def extsetup():
  >     print('ambigrel.s=%s' % ambigrel.s)
  > EOF
  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}/libroot; hg --config extensions.loadrel=loadrel.py log -r null -T "foo\n")
  ambigrel.s=libroot/mod/ambig.py
  foo
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
  >     ui.flush()
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
  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}; hg --config extensions.extroot=$TESTTMP/extroot log -r null -T 'foo\n')
  (extroot) from extroot.bar import *: this is extroot.bar
  (extroot) import extroot.sub1.baz: this is extroot.sub1.baz
  (extroot) import extroot: this is extroot.__init__
  (extroot) from extroot.bar import s: this is extroot.bar
  (extroot) import extroot.bar in func(): this is extroot.bar
  foo

#if no-py3k
  $ rm "$TESTTMP"/extroot/foo.*
  $ rm -Rf "$TESTTMP/extroot/__pycache__"
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
  $ hg --config extensions.extroot=$TESTTMP/extroot log -r null -T "foo\n"
  (extroot) from bar import *: this is extroot.bar
  (extroot) import sub1.baz: this is extroot.sub1.baz
  (extroot) import sub1: this is extroot.sub1.__init__
  (extroot) from bar import s: this is extroot.bar
  (extroot) import bar in func(): this is extroot.bar
  foo
#endif

#if demandimport

Examine whether module loading is delayed until actual referring, even
though module is imported with "absolute_import" feature.

Files below in each packages are used for described purpose:

- "called": examine whether "from MODULE import ATTR" works correctly
- "unused": examine whether loading is delayed correctly
- "used":   examine whether "from PACKAGE import MODULE" works correctly

Package hierarchy is needed to examine whether demand importing works
as expected for "from SUB.PACK.AGE import MODULE".

Setup "external library" to be imported with "absolute_import"
feature.

  $ mkdir -p $TESTTMP/extlibroot/lsub1/lsub2
  $ touch $TESTTMP/extlibroot/__init__.py
  $ touch $TESTTMP/extlibroot/lsub1/__init__.py
  $ touch $TESTTMP/extlibroot/lsub1/lsub2/__init__.py

  $ cat > $TESTTMP/extlibroot/lsub1/lsub2/called.py <<EOF
  > def func():
  >     return "this is extlibroot.lsub1.lsub2.called.func()"
  > EOF
  $ cat > $TESTTMP/extlibroot/lsub1/lsub2/unused.py <<EOF
  > raise Exception("extlibroot.lsub1.lsub2.unused is loaded unintentionally")
  > EOF
  $ cat > $TESTTMP/extlibroot/lsub1/lsub2/used.py <<EOF
  > detail = "this is extlibroot.lsub1.lsub2.used"
  > EOF

Setup sub-package of "external library", which causes instantiation of
demandmod in "recurse down the module chain" code path. Relative
importing with "absolute_import" feature isn't tested, because "level
>=1 " doesn't cause instantiation of demandmod.

  $ mkdir -p $TESTTMP/extlibroot/recursedown/abs
  $ cat > $TESTTMP/extlibroot/recursedown/abs/used.py <<EOF
  > detail = "this is extlibroot.recursedown.abs.used"
  > EOF
  $ cat > $TESTTMP/extlibroot/recursedown/abs/__init__.py <<EOF
  > from __future__ import absolute_import
  > from extlibroot.recursedown.abs.used import detail
  > EOF

  $ mkdir -p $TESTTMP/extlibroot/recursedown/legacy
  $ cat > $TESTTMP/extlibroot/recursedown/legacy/used.py <<EOF
  > detail = "this is extlibroot.recursedown.legacy.used"
  > EOF
  $ cat > $TESTTMP/extlibroot/recursedown/legacy/__init__.py <<EOF
  > # legacy style (level == -1) import
  > from extlibroot.recursedown.legacy.used import detail
  > EOF

  $ cat > $TESTTMP/extlibroot/recursedown/__init__.py <<EOF
  > from __future__ import absolute_import
  > from extlibroot.recursedown.abs import detail as absdetail
  > from .legacy import detail as legacydetail
  > EOF

Setup package that re-exports an attribute of its submodule as the same
name. This leaves 'shadowing.used' pointing to 'used.detail', but still
the submodule 'used' should be somehow accessible. (issue5617)

  $ mkdir -p $TESTTMP/extlibroot/shadowing
  $ cat > $TESTTMP/extlibroot/shadowing/used.py <<EOF
  > detail = "this is extlibroot.shadowing.used"
  > EOF
  $ cat > $TESTTMP/extlibroot/shadowing/proxied.py <<EOF
  > from __future__ import absolute_import
  > from extlibroot.shadowing.used import detail
  > EOF
  $ cat > $TESTTMP/extlibroot/shadowing/__init__.py <<EOF
  > from __future__ import absolute_import
  > from .used import detail as used
  > EOF

Setup extension local modules to be imported with "absolute_import"
feature.

  $ mkdir -p $TESTTMP/absextroot/xsub1/xsub2
  $ touch $TESTTMP/absextroot/xsub1/__init__.py
  $ touch $TESTTMP/absextroot/xsub1/xsub2/__init__.py

  $ cat > $TESTTMP/absextroot/xsub1/xsub2/called.py <<EOF
  > def func():
  >     return "this is absextroot.xsub1.xsub2.called.func()"
  > EOF
  $ cat > $TESTTMP/absextroot/xsub1/xsub2/unused.py <<EOF
  > raise Exception("absextroot.xsub1.xsub2.unused is loaded unintentionally")
  > EOF
  $ cat > $TESTTMP/absextroot/xsub1/xsub2/used.py <<EOF
  > detail = "this is absextroot.xsub1.xsub2.used"
  > EOF

Setup extension local modules to examine whether demand importing
works as expected in "level > 1" case.

  $ cat > $TESTTMP/absextroot/relimportee.py <<EOF
  > detail = "this is absextroot.relimportee"
  > EOF
  $ cat > $TESTTMP/absextroot/xsub1/xsub2/relimporter.py <<EOF
  > from __future__ import absolute_import
  > from ... import relimportee
  > detail = "this relimporter imports %r" % (relimportee.detail)
  > EOF

Setup modules, which actually import extension local modules at
runtime.

  $ cat > $TESTTMP/absextroot/absolute.py << EOF
  > from __future__ import absolute_import
  > 
  > # import extension local modules absolutely (level = 0)
  > from absextroot.xsub1.xsub2 import used, unused
  > from absextroot.xsub1.xsub2.called import func
  > 
  > def getresult():
  >     result = []
  >     result.append(used.detail)
  >     result.append(func())
  >     return result
  > EOF

  $ cat > $TESTTMP/absextroot/relative.py << EOF
  > from __future__ import absolute_import
  > 
  > # import extension local modules relatively (level == 1)
  > from .xsub1.xsub2 import used, unused
  > from .xsub1.xsub2.called import func
  > 
  > # import a module, which implies "importing with level > 1"
  > from .xsub1.xsub2 import relimporter
  > 
  > def getresult():
  >     result = []
  >     result.append(used.detail)
  >     result.append(func())
  >     result.append(relimporter.detail)
  >     return result
  > EOF

Setup main procedure of extension.

  $ cat > $TESTTMP/absextroot/__init__.py <<EOF
  > from __future__ import absolute_import
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > # "absolute" and "relative" shouldn't be imported before actual
  > # command execution, because (1) they import same modules, and (2)
  > # preceding import (= instantiate "demandmod" object instead of
  > # real "module" object) might hide problem of succeeding import.
  > 
  > @command(b'showabsolute', [], norepo=True)
  > def showabsolute(ui, *args, **opts):
  >     from absextroot import absolute
  >     ui.write('ABS: %s\n' % '\nABS: '.join(absolute.getresult()))
  > 
  > @command(b'showrelative', [], norepo=True)
  > def showrelative(ui, *args, **opts):
  >     from . import relative
  >     ui.write('REL: %s\n' % '\nREL: '.join(relative.getresult()))
  > 
  > # import modules from external library
  > from extlibroot.lsub1.lsub2 import used as lused, unused as lunused
  > from extlibroot.lsub1.lsub2.called import func as lfunc
  > from extlibroot.recursedown import absdetail, legacydetail
  > from extlibroot.shadowing import proxied
  > 
  > def uisetup(ui):
  >     result = []
  >     result.append(lused.detail)
  >     result.append(lfunc())
  >     result.append(absdetail)
  >     result.append(legacydetail)
  >     result.append(proxied.detail)
  >     ui.write('LIB: %s\n' % '\nLIB: '.join(result))
  > EOF

Examine module importing.

  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}; hg --config extensions.absextroot=$TESTTMP/absextroot showabsolute)
  LIB: this is extlibroot.lsub1.lsub2.used
  LIB: this is extlibroot.lsub1.lsub2.called.func()
  LIB: this is extlibroot.recursedown.abs.used
  LIB: this is extlibroot.recursedown.legacy.used
  LIB: this is extlibroot.shadowing.used
  ABS: this is absextroot.xsub1.xsub2.used
  ABS: this is absextroot.xsub1.xsub2.called.func()

  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}; hg --config extensions.absextroot=$TESTTMP/absextroot showrelative)
  LIB: this is extlibroot.lsub1.lsub2.used
  LIB: this is extlibroot.lsub1.lsub2.called.func()
  LIB: this is extlibroot.recursedown.abs.used
  LIB: this is extlibroot.recursedown.legacy.used
  LIB: this is extlibroot.shadowing.used
  REL: this is absextroot.xsub1.xsub2.used
  REL: this is absextroot.xsub1.xsub2.called.func()
  REL: this relimporter imports 'this is absextroot.relimportee'

Examine whether sub-module is imported relatively as expected.

See also issue5208 for detail about example case on Python 3.x.

  $ f -q $TESTTMP/extlibroot/lsub1/lsub2/notexist.py
  $TESTTMP/extlibroot/lsub1/lsub2/notexist.py: file not found

  $ cat > $TESTTMP/notexist.py <<EOF
  > text = 'notexist.py at root is loaded unintentionally\n'
  > EOF

  $ cat > $TESTTMP/checkrelativity.py <<EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > 
  > # demand import avoids failure of importing notexist here
  > import extlibroot.lsub1.lsub2.notexist
  > 
  > @command(b'checkrelativity', [], norepo=True)
  > def checkrelativity(ui, *args, **opts):
  >     try:
  >         ui.write(extlibroot.lsub1.lsub2.notexist.text)
  >         return 1 # unintentional success
  >     except ImportError:
  >         pass # intentional failure
  > EOF

  $ (PYTHONPATH=${PYTHONPATH}${PATHSEP}${TESTTMP}; hg --config extensions.checkrelativity=$TESTTMP/checkrelativity.py checkrelativity)

#endif

Make sure a broken uisetup doesn't globally break hg:
  $ cat > $TESTTMP/baduisetup.py <<EOF
  > def uisetup(ui):
  >     1/0
  > EOF

Even though the extension fails during uisetup, hg is still basically usable:
  $ hg --config extensions.baduisetup=$TESTTMP/baduisetup.py version
  Traceback (most recent call last):
    File "*/mercurial/extensions.py", line *, in _runuisetup (glob)
      uisetup(ui)
    File "$TESTTMP/baduisetup.py", line 2, in uisetup
      1/0
  ZeroDivisionError: integer division or modulo by zero
  *** failed to set up extension baduisetup: integer division or modulo by zero
  Mercurial Distributed SCM (version *) (glob)
  (see https://mercurial-scm.org for more information)
  
  Copyright (C) 2005-2017 Matt Mackall and others
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

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
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'debugfoobar', [], 'hg debugfoobar')
  > def debugfoobar(ui, repo, *args, **opts):
  >     "yet another debug command"
  >     pass
  > @command(b'foo', [], 'hg foo')
  > def foo(ui, repo, *args, **opts):
  >     """yet another foo command
  >     This command has been DEPRECATED since forever.
  >     """
  >     pass
  > EOF
  $ debugpath=`pwd`/debugextension.py
  $ echo "debugextension = $debugpath" >> $HGRCPATH

  $ hg help debugextension
  alias for: debugextensions
  
  hg debugextensions
  
  show information about active extensions
  
  Options:
  
    --excludedefault exclude extensions marked as default-on
  
  (some details hidden, use --verbose to show complete help)


  $ hg --verbose help debugextension
  alias for: debugextensions
  
  hg debugextensions
  
  show information about active extensions
  
  Options:
  
      --excludedefault    exclude extensions marked as default-on
   -T --template TEMPLATE display with template (EXPERIMENTAL)
  
  Global options ([+] can be repeated):
  
   -R --repository REPO     repository root directory or name of overlay bundle
                            file
      --cwd DIR             change working directory
   -y --noninteractive      do not prompt, automatically pick the first choice
                            for all prompts
   -q --quiet               suppress output
   -v --verbose             enable additional output
      --color TYPE          when to colorize (boolean, always, auto, never, or
                            debug)
      --config CONFIG [+]   set/override config option (use
                            'section.name=value')
      --configfile FILE [+] enables the given config file
      --debug               enable debugging output
      --debugger            start debugger
      --encoding ENCODE     set the charset encoding (default: ascii)
      --encodingmode MODE   set the charset encoding mode (default: strict)
      --traceback           always print a traceback on exception
      --time                time how long the command takes
      --profile             print command execution profile
      --version             output version information and exit
   -h --help                display help and exit
      --hidden              consider hidden changesets
      --pager TYPE          when to paginate (boolean, always, auto, or never)
                            (default: auto)






  $ hg --debug help debugextension
  alias for: debugextensions
  
  hg debugextensions
  
  show information about active extensions
  
  Options:
  
      --excludedefault    exclude extensions marked as default-on
   -T --template TEMPLATE display with template (EXPERIMENTAL)
  
  Global options ([+] can be repeated):
  
   -R --repository REPO     repository root directory or name of overlay bundle
                            file
      --cwd DIR             change working directory
   -y --noninteractive      do not prompt, automatically pick the first choice
                            for all prompts
   -q --quiet               suppress output
   -v --verbose             enable additional output
      --color TYPE          when to colorize (boolean, always, auto, never, or
                            debug)
      --config CONFIG [+]   set/override config option (use
                            'section.name=value')
      --configfile FILE [+] enables the given config file
      --debug               enable debugging output
      --debugger            start debugger
      --encoding ENCODE     set the charset encoding (default: ascii)
      --encodingmode MODE   set the charset encoding mode (default: strict)
      --traceback           always print a traceback on exception
      --time                time how long the command takes
      --profile             print command execution profile
      --version             output version information and exit
   -h --help                display help and exit
      --hidden              consider hidden changesets
      --pager TYPE          when to paginate (boolean, always, auto, or never)
                            (default: auto)





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
  
  (use 'hg help -e extdiff' to show help for the extdiff extension)
  
  Options ([+] can be repeated):
  
   -p --program CMD         comparison program to run
   -o --option OPT [+]      pass option to comparison program
   -r --rev REV [+]         revision
   -c --change REV          change made by revision
      --patch               compare patches for two revisions
   -I --include PATTERN [+] include names matching the given patterns
   -X --exclude PATTERN [+] exclude names matching the given patterns
  
  (some details hidden, use --verbose to show complete help)










  $ hg help --extension extdiff
  extdiff extension - command to allow external programs to compare revisions
  
  The extdiff Mercurial extension allows you to use external programs to compare
  revisions, or revision with working directory. The external diff programs are
  called with a configurable set of options and two non-option arguments: paths
  to directories containing snapshots of files to compare.
  
  The extdiff extension also allows you to configure new diff commands, so you
  do not need to type 'hg extdiff -p kdiff3' always.
  
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
  
  You can use -I/-X and list of file or directory names like normal 'hg diff'
  command. The extdiff extension makes snapshots of only needed files, so
  running the external diff program will actually be pretty fast (at least
  faster than having to compare the entire tree).
  
  Commands:
  
   extdiff       use external program to diff repository (or selected files)
















  $ echo 'extdiff = !' >> $HGRCPATH

Test help topic with same name as extension

  $ cat > multirevs.py <<EOF
  > from edenscm.mercurial import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > """multirevs extension
  > Big multi-line module docstring."""
  > @command(b'multirevs', [], 'ARG', norepo=True)
  > def multirevs(ui, repo, arg, *args, **opts):
  >     """multirevs command"""
  >     pass
  > EOF
  $ echo "multirevs = multirevs.py" >> $HGRCPATH

  $ hg help multirevs | tail
        used):
  
          hg update :@
  
      - Show diff between tags 1.3 and 1.5 (this works because the first and the
        last revisions of the revset are used):
  
          hg diff -r 1.3::1.5
  
  use 'hg help -c multirevs' to see help for the multirevs command






  $ hg help -c multirevs
  hg multirevs ARG
  
  multirevs command
  
  (some details hidden, use --verbose to show complete help)



  $ hg multirevs
  hg multirevs: invalid arguments
  (use 'hg multirevs -h' to get help)
  [255]



  $ echo "multirevs = !" >> $HGRCPATH

For extensions, which name matches one of its commands, help
message should ask '-v -e' to get list of built-in aliases
along with extension help itself

  $ mkdir $TESTTMP/d
  $ cat > $TESTTMP/d/dodo.py <<EOF
  > """
  > This is an awesome 'dodo' extension. It does nothing and
  > writes 'Foo foo'
  > """
  > from edenscm.mercurial import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'dodo', [], 'hg dodo')
  > def dodo(ui, *args, **kwargs):
  >     """Does nothing"""
  >     ui.write("I do nothing. Yay\\n")
  > @command(b'foofoo', [], 'hg foofoo')
  > def foofoo(ui, *args, **kwargs):
  >     """Writes 'Foo foo'"""
  >     ui.write("Foo foo\\n")
  > EOF
  $ dodopath=$TESTTMP/d/dodo.py

  $ echo "dodo = $dodopath" >> $HGRCPATH

Make sure that '-e' prints help for the extension.
  $ hg help -e dodo
  dodo extension -
  
  This is an awesome 'dodo' extension. It does nothing and writes 'Foo foo'
  
  Commands:
  
   dodo          Does nothing
   foofoo        Writes 'Foo foo'

Make sure that '-v -e' prints help for the extension.
  $ hg help -v -e dodo
  dodo extension -
  
  This is an awesome 'dodo' extension. It does nothing and writes 'Foo foo'
  
  Commands:
  
   dodo          Does nothing
   foofoo        Writes 'Foo foo'

Make sure that single '-v' option shows help and global options for the 'dodo' command
  $ hg help -v dodo
  hg dodo
  
  Does nothing
  
  (use 'hg help -e dodo' to show help for the dodo extension)
  
  Global options ([+] can be repeated):
  
   -R --repository REPO     repository root directory or name of overlay bundle
                            file
      --cwd DIR             change working directory
   -y --noninteractive      do not prompt, automatically pick the first choice
                            for all prompts
   -q --quiet               suppress output
   -v --verbose             enable additional output
      --color TYPE          when to colorize (boolean, always, auto, never, or
                            debug)
      --config CONFIG [+]   set/override config option (use
                            'section.name=value')
      --configfile FILE [+] enables the given config file
      --debug               enable debugging output
      --debugger            start debugger
      --encoding ENCODE     set the charset encoding (default: ascii)
      --encodingmode MODE   set the charset encoding mode (default: strict)
      --traceback           always print a traceback on exception
      --time                time how long the command takes
      --profile             print command execution profile
      --version             output version information and exit
   -h --help                display help and exit
      --hidden              consider hidden changesets
      --pager TYPE          when to paginate (boolean, always, auto, or never)
                            (default: auto)

In case when extension name doesn't match any of its commands,
help message should ask for '-v' to get list of built-in aliases
along with extension help
  $ cat > $TESTTMP/d/dudu.py <<EOF
  > """
  > This is an awesome 'dudu' extension. It does something and
  > also writes 'Beep beep'
  > """
  > from edenscm.mercurial import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command(b'something', [], 'hg something')
  > def something(ui, *args, **kwargs):
  >     """Does something"""
  >     ui.write("I do something. Yaaay\\n")
  > @command(b'beep', [], 'hg beep')
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
  
  Commands:
  
   beep          Writes 'Beep beep'
   something     Does something

In case when extension name doesn't match any of its commands,
help options '-v' and '-v -e' should be equivalent
  $ hg help -v dudu
  dudu extension -
  
  This is an awesome 'dudu' extension. It does something and also writes 'Beep
  beep'
  
  Commands:
  
   beep          Writes 'Beep beep'
   something     Does something

  $ hg help -v -e dudu
  dudu extension -
  
  This is an awesome 'dudu' extension. It does something and also writes 'Beep
  beep'
  
  Commands:
  
   beep          Writes 'Beep beep'
   something     Does something

Disabled extension commands:

  $ ORGHGRCPATH=$HGRCPATH
  $ HGRCPATH=
  $ export HGRCPATH
  $ hg churn
  unknown command 'churn'
  (use 'hg help' to get help)
  [255]



Disabled extensions:

  $ hg help churn
  churn extension - command to display statistics about repository history
  
  (use 'hg help extensions' for information on enabling extensions)

Broken disabled extension and command:
(There is no way to change "edenscm.hgext" path so the extensions here will not
get scanned)

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

  $ hg --config extensions.path=./path.py help broken 2>&1 | grep -v "failed to import"
  abort: no such help topic: broken
  (try 'hg help --keyword broken')


  $ cat > hgext/forest.py <<EOF
  > cmdtable = None
  > EOF
  $ hg --config extensions.path=./path.py help foo 2>&1 | grep -v "failed to import"
  abort: no such help topic: foo
  (try 'hg help --keyword foo')

  $ cat > throw.py <<EOF
  > from edenscm.mercurial import commands, registrar, util
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > class Bogon(Exception): pass
  > @command(b'throw', [], 'hg throw', norepo=True)
  > def throw(ui, **opts):
  >     """throws an exception"""
  >     raise Bogon()
  > EOF

No declared supported version, extension complains:
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** Mercurial Distributed SCM * (glob)

empty declaration of supported version, extension complains:
  $ echo "testedwith = ''" >> throw.py
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** * has crashed: (glob)

If the extension specifies a buglink, show that:
  $ echo 'buglink = "http://example.com/bts"' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*'
  ** * has crashed: (glob)

If the extensions declare outdated versions, accuse the older extension first:
  $ echo "from edenscm.mercurial import util" >> older.py
  $ echo "util.version = lambda:'2.2'" >> older.py
  $ echo "testedwith = '1.9.3'" >> older.py
  $ echo "testedwith = '2.1.1'" >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*' | grep -v 'possibly-broken' | grep -v 'Please disable'
  ** * has crashed: (glob)

One extension only tested with older, one only with newer versions:
  $ echo "util.version = lambda:'2.1'" >> older.py
  $ rm -f older.pyc older.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*' | grep -v 'possibly-broken' | grep -v 'Please disable'
  ** * has crashed: (glob)

Older extension is tested with current version, the other only with newer:
  $ echo "util.version = lambda:'1.9.3'" >> older.py
  $ rm -f older.pyc older.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py --config extensions.older=older.py \
  >   throw 2>&1 | egrep '^\*\*' | grep -v 'possibly-broken' | grep -v 'Please disable'
  ** * has crashed: (glob)

Declare the version as supporting this hg version, show regular bts link:
  $ hgver=`hg debuginstall -T '{hgver}'`
  $ echo 'testedwith = """'"$hgver"'"""' >> throw.py
  $ if [ -z "$hgver" ]; then
  >   echo "unable to fetch a mercurial version. Make sure __version__ is correct";
  > fi
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*' | grep -v 'possibly-broken' | grep -v 'Please disable'
  ** * has crashed: (glob)

Patch version is ignored during compatibility check
  $ echo "testedwith = '3.2'" >> throw.py
  $ echo "util.version = lambda:'3.2.2'" >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg --config extensions.throw=throw.py throw 2>&1 | egrep '^\*\*' | grep -v 'possibly-broken' | grep -v 'Please disable'
  ** * has crashed: (glob)

Test version number support in 'hg version':
  $ echo '__version__ = (1, 2, 3)' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg version -v | grep -v external
  Mercurial Distributed SCM (version *) (glob)
  (see https://mercurial-scm.org for more information)
  
  Copyright (C) 2005-* Matt Mackall and others (glob)
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.
  
  Enabled extensions:
  

  $ hg version -v --config extensions.throw=throw.py | egrep '(^external|throw)'
    throw\s* external  1.2.3 (re)
  $ echo 'getversion = lambda: "1.twentythree"' >> throw.py
  $ rm -f throw.pyc throw.pyo
  $ rm -Rf __pycache__
  $ hg version -v --config extensions.throw=throw.py | egrep 'throw'
    throw \s* external  1.twentythree (re)

  $ hg version -q --config extensions.throw=throw.py
  Mercurial Distributed SCM (version *) (glob)

Test JSON output of version:

  $ hg version -Tjson
  [
   {
    "extensions": [*], (glob)
    "ver": "*" (glob)
   }
  ]

  $ hg version --config extensions.throw=throw.py -Tjson
  [
   {
    "extensions": [{"bundled": false, "name": "throw", "ver": "1.twentythree"}, *], (glob)
    "ver": "3.2.2"
   }
  ]

  $ hg version -Tjson
  [
   {
    "extensions": [{"bundled": false, "name": "conflictinfo", "ver": null}, *], (glob)
    "ver": "*" (glob)
   }
  ]

Test template output of version:

  $ hg version --config extensions.throw=throw.py --config extensions.journal= \
  > -T'{extensions % "{pad(name, 8)}  {pad(ver, 16)}  ({if(bundled, "internal", "external")})\n"}' | egrep '(throw|journal)'
  throw     1.twentythree     (external)
  journal                     (internal)

Refuse to load extensions with minimum version requirements

  $ cat > minversion1.py << EOF
  > from edenscm.mercurial import util
  > util.version = lambda: '3.5.2'
  > minimumhgversion = '3.6'
  > EOF
  $ hg --config extensions.minversion=minversion1.py version
  (third party extension minversion requires version 3.6 or newer of Mercurial; disabling)
  Mercurial Distributed SCM (version 3.5.2)
  (see https://mercurial-scm.org for more information)
  
  Copyright (C) 2005-* Matt Mackall and others (glob)
  This is free software; see the source for copying conditions. There is NO
  warranty; not even for MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.

  $ cat > minversion2.py << EOF
  > from edenscm.mercurial import util
  > util.version = lambda: '3.6'
  > minimumhgversion = '3.7'
  > EOF
  $ hg --config extensions.minversion=minversion2.py version 2>&1 | egrep '\(third'
  (third party extension minversion requires version 3.7 or newer of Mercurial; disabling)

Can load version that is only off by point release

  $ cat > minversion2.py << EOF
  > from edenscm.mercurial import util
  > util.version = lambda: '3.6.1'
  > minimumhgversion = '3.6'
  > EOF
  $ hg --config extensions.minversion=minversion3.py version 2>&1 | egrep '\(third'
  [1]

Can load minimum version identical to current

  $ cat > minversion3.py << EOF
  > from edenscm.mercurial import util
  > util.version = lambda: '3.5'
  > minimumhgversion = '3.5'
  > EOF
  $ hg --config extensions.minversion=minversion3.py version 2>&1 | egrep '\(third'
  [1]

Restore HGRCPATH

  $ HGRCPATH=$ORGHGRCPATH
  $ export HGRCPATH

Commands handling multiple repositories at a time should invoke only
"reposetup()" of extensions enabling in the target repository.

  $ mkdir reposetup-test
  $ cd reposetup-test

  $ cat > $TESTTMP/reposetuptest.py <<EOF
  > from edenscm.mercurial import extensions
  > def reposetup(ui, repo):
  >     ui.write('reposetup() for %s\n' % (repo.root))
  >     ui.flush()
  > EOF
  $ hg init src
  $ echo a > src/a
  $ hg -R src commit -Am '#0 at src/a'
  adding a
  $ echo '[extensions]' >> src/.hg/hgrc
  $ echo '# enable extension locally' >> src/.hg/hgrc
  $ echo "reposetuptest = $TESTTMP/reposetuptest.py" >> src/.hg/hgrc
  $ hg -R src status
  reposetup() for $TESTTMP/reposetup-test/src
  reposetup() for $TESTTMP/reposetup-test/src (chg !)

  $ hg clone -U src clone-dst1
  reposetup() for $TESTTMP/reposetup-test/src
  $ hg init push-dst1
  $ hg -q -R src push push-dst1
  reposetup() for $TESTTMP/reposetup-test/src
  $ hg init pull-src1
  $ hg -q -R pull-src1 pull src
  reposetup() for $TESTTMP/reposetup-test/src

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > # disable extension globally and explicitly
  > reposetuptest = !
  > EOF
  $ hg clone -U src clone-dst2
  reposetup() for $TESTTMP/reposetup-test/src
  $ hg init push-dst2
  $ hg -q -R src push push-dst2
  reposetup() for $TESTTMP/reposetup-test/src
  $ hg init pull-src2
  $ hg -q -R pull-src2 pull src
  reposetup() for $TESTTMP/reposetup-test/src

  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > # enable extension globally
  > reposetuptest = $TESTTMP/reposetuptest.py
  > EOF
  $ hg clone -U src clone-dst3
  reposetup() for $TESTTMP/reposetup-test/src
  reposetup() for $TESTTMP/reposetup-test/clone-dst3
  $ hg init push-dst3
  reposetup() for $TESTTMP/reposetup-test/push-dst3
  $ hg -q -R src push push-dst3
  reposetup() for $TESTTMP/reposetup-test/src
  reposetup() for $TESTTMP/reposetup-test/push-dst3
  $ hg init pull-src3
  reposetup() for $TESTTMP/reposetup-test/pull-src3
  $ hg -q -R pull-src3 pull src
  reposetup() for $TESTTMP/reposetup-test/pull-src3
  reposetup() for $TESTTMP/reposetup-test/src

  $ echo '[extensions]' >> src/.hg/hgrc
  $ echo '# disable extension locally' >> src/.hg/hgrc
  $ echo 'reposetuptest = !' >> src/.hg/hgrc
  $ hg clone -U src clone-dst4
  reposetup() for $TESTTMP/reposetup-test/clone-dst4
  $ hg init push-dst4
  reposetup() for $TESTTMP/reposetup-test/push-dst4
  $ hg -q -R src push push-dst4
  reposetup() for $TESTTMP/reposetup-test/push-dst4
  $ hg init pull-src4
  reposetup() for $TESTTMP/reposetup-test/pull-src4
  $ hg -q -R pull-src4 pull src
  reposetup() for $TESTTMP/reposetup-test/pull-src4

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
  $ hg -R parent status -A
  reposetup() for $TESTTMP/reposetup-test/parent
  C .hgsub

  $ cd ..

Prohibit registration of commands that don't use @command (issue5137)

  $ hg init deprecated
  $ cd deprecated

  $ cat <<EOF > deprecatedcmd.py
  > def deprecatedcmd(repo, ui):
  >     pass
  > cmdtable = {
  >     'deprecatedcmd': (deprecatedcmd, [], ''),
  > }
  > EOF
  $ cat <<EOF > .hg/hgrc
  > [extensions]
  > deprecatedcmd = `pwd`/deprecatedcmd.py
  > mq = !
  > hgext.mq = !
  > hgext/mq = !
  > EOF

  $ hg deprecatedcmd > /dev/null
  *** failed to import extension deprecatedcmd from $TESTTMP/deprecated/deprecatedcmd.py: missing attributes: norepo, optionalrepo, inferrepo
  *** (use @command decorator to register 'deprecatedcmd')
  unknown command 'deprecatedcmd'
  (use 'hg help' to get help)
  [255]

 the extension shouldn't be loaded at all so the mq works:

  $ hg log -r null --config extensions.mq= > /dev/null
  *** failed to import extension deprecatedcmd from $TESTTMP/deprecated/deprecatedcmd.py: missing attributes: norepo, optionalrepo, inferrepo
  *** (use @command decorator to register 'deprecatedcmd')

  $ cd ..

Test synopsis and docstring extending

  $ hg init exthelp
  $ cat > exthelp.py <<EOF
  > from edenscm.mercurial import commands, extensions
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
  $ cd ..

Show deprecation warning for the use of cmdutil.command

  $ cat > nonregistrar.py <<EOF
  > from edenscm.mercurial import cmdutil
  > cmdtable = {}
  > command = cmdutil.command(cmdtable)
  > @command(b'foo', [], norepo=True)
  > def foo(ui):
  >     pass
  > EOF

  $ hg --config extensions.nonregistrar=`pwd`/nonregistrar.py version > /dev/null
  devel-warn: cmdutil.command is deprecated, use registrar.command to register 'foo'
  (compatibility will be dropped after Mercurial-4.6, update your code.) * (glob)

Prohibit the use of unicode strings as the default value of options

  $ hg init $TESTTMP/opt-unicode-default

  $ cat > $TESTTMP/test_unicode_default_value.py << EOF
  > from edenscm.mercurial import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('dummy', [('', 'opt', u'value', u'help')], 'ext [OPTIONS]')
  > def ext(*args, **opts):
  >     print(opts['opt'])
  > EOF
  $ cat > $TESTTMP/opt-unicode-default/.hg/hgrc << EOF
  > [extensions]
  > test_unicode_default_value = $TESTTMP/test_unicode_default_value.py
  > EOF
  $ hg -R $TESTTMP/opt-unicode-default dummy
  *** failed to import extension test_unicode_default_value from $TESTTMP/test_unicode_default_value.py: option 'dummy.opt' has a unicode default value
  *** (change the dummy.opt default value to a non-unicode string)
  unknown command 'dummy'
  (use 'hg help' to get help)
  [255]
