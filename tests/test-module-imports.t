#require test-repo

  $ import_checker="$TESTDIR"/../contrib/import-checker.py

Run the doctests from the import checker, and make sure
it's working correctly.
  $ TERM=dumb
  $ export TERM
  $ python -m doctest $import_checker

  $ cd "$TESTDIR"/..

There are a handful of cases here that require renaming a module so it
doesn't overlap with a stdlib module name. There are also some cycles
here that we should still endeavor to fix, and some cycles will be
hidden by deduplication algorithm in the cycle detector, so fixing
these may expose other cycles.

  $ hg locate 'mercurial/**.py' 'hgext/**.py' | sed 's-\\-/-g' | python "$import_checker" -
  mercurial/dispatch.py mixed imports
     stdlib:    commands
     relative:  error, extensions, fancyopts, hg, hook, util
  mercurial/fileset.py mixed imports
     stdlib:    parser
     relative:  error, merge, util
  mercurial/revset.py mixed imports
     stdlib:    parser
     relative:  error, hbisect, phases, util
  mercurial/templater.py mixed imports
     stdlib:    parser
     relative:  config, error, templatefilters, templatekw, util
  mercurial/ui.py mixed imports
     stdlib:    formatter
     relative:  config, error, scmutil, util
  Import cycle: mercurial.cmdutil -> mercurial.context -> mercurial.subrepo -> mercurial.cmdutil
  Import cycle: hgext.largefiles.basestore -> hgext.largefiles.localstore -> hgext.largefiles.basestore
  Import cycle: mercurial.commands -> mercurial.commandserver -> mercurial.dispatch -> mercurial.commands
