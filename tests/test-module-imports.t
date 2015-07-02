#require test-repo

  $ import_checker="$TESTDIR"/../contrib/import-checker.py

Run the doctests from the import checker, and make sure
it's working correctly.
  $ TERM=dumb
  $ export TERM
  $ python -m doctest $import_checker

Run additional tests for the import checker

  $ mkdir testpackage

  $ cat > testpackage/multiple.py << EOF
  > from __future__ import absolute_import
  > import os, sys
  > EOF

  $ cat > testpackage/unsorted.py << EOF
  > from __future__ import absolute_import
  > import sys
  > import os
  > EOF

  $ cat > testpackage/stdafterlocal.py << EOF
  > from __future__ import absolute_import
  > from . import unsorted
  > import os
  > EOF

  $ cat > testpackage/requirerelative.py << EOF
  > from __future__ import absolute_import
  > import testpackage.unsorted
  > EOF

  $ cat > testpackage/importalias.py << EOF
  > from __future__ import absolute_import
  > import ui
  > EOF

  $ cat > testpackage/relativestdlib.py << EOF
  > from __future__ import absolute_import
  > from .. import os
  > EOF

  $ cat > testpackage/symbolimport.py << EOF
  > from __future__ import absolute_import
  > from .unsorted import foo
  > EOF

  $ cat > testpackage/latesymbolimport.py << EOF
  > from __future__ import absolute_import
  > from . import unsorted
  > from mercurial.node import hex
  > EOF

  $ cat > testpackage/multiplegroups.py << EOF
  > from __future__ import absolute_import
  > from . import unsorted
  > from . import more
  > EOF

  $ mkdir testpackage/subpackage
  $ cat > testpackage/subpackage/levelpriority.py << EOF
  > from __future__ import absolute_import
  > from . import foo
  > from .. import parent
  > EOF

  $ cat > testpackage/sortedentries.py << EOF
  > from __future__ import absolute_import
  > from . import (
  >     foo,
  >     bar,
  > )
  > EOF

  $ cat > testpackage/importfromalias.py << EOF
  > from __future__ import absolute_import
  > from . import ui
  > EOF

  $ cat > testpackage/importfromrelative.py << EOF
  > from __future__ import absolute_import
  > from testpackage.unsorted import foo
  > EOF

  $ python "$import_checker" testpackage/*.py testpackage/subpackage/*.py
  testpackage/importalias.py ui module must be "as" aliased to uimod
  testpackage/importfromalias.py ui from testpackage must be "as" aliased to uimod
  testpackage/importfromrelative.py import should be relative: testpackage.unsorted
  testpackage/importfromrelative.py direct symbol import from testpackage.unsorted
  testpackage/latesymbolimport.py symbol import follows non-symbol import: mercurial.node
  testpackage/multiple.py multiple imported names: os, sys
  testpackage/multiplegroups.py multiple "from . import" statements
  testpackage/relativestdlib.py relative import of stdlib module
  testpackage/requirerelative.py import should be relative: testpackage.unsorted
  testpackage/sortedentries.py imports from testpackage not lexically sorted: bar < foo
  testpackage/stdafterlocal.py stdlib import follows local import: os
  testpackage/subpackage/levelpriority.py higher-level import should come first: testpackage
  testpackage/symbolimport.py direct symbol import from testpackage.unsorted
  testpackage/unsorted.py imports not lexically sorted: os < sys
  [1]

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
     relative:  config, error, progress, scmutil, util
  Import cycle: mercurial.cmdutil -> mercurial.context -> mercurial.subrepo -> mercurial.cmdutil
  Import cycle: hgext.largefiles.basestore -> hgext.largefiles.localstore -> hgext.largefiles.basestore
  Import cycle: mercurial.commands -> mercurial.commandserver -> mercurial.dispatch -> mercurial.commands
  [1]
