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

  $ touch testpackage/subpackage/foo.py
  $ cat > testpackage/subpackage/__init__.py << EOF
  > from __future__ import absolute_import
  > from . import levelpriority  # should not cause cycle
  > EOF

  $ cat > testpackage/subpackage/localimport.py << EOF
  > from __future__ import absolute_import
  > from . import foo
  > def bar():
  >     # should not cause "higher-level import should come first"
  >     from .. import unsorted
  >     # but other errors should be detected
  >     from .. import more
  >     import testpackage.subpackage.levelpriority
  > EOF

  $ cat > testpackage/importmodulefromsub.py << EOF
  > from __future__ import absolute_import
  > from .subpackage import foo  # not a "direct symbol import"
  > EOF

  $ cat > testpackage/importsymbolfromsub.py << EOF
  > from __future__ import absolute_import
  > from .subpackage import foo, nonmodule
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
  testpackage/importalias.py:2: ui module must be "as" aliased to uimod
  testpackage/importfromalias.py:2: ui from testpackage must be "as" aliased to uimod
  testpackage/importfromrelative.py:2: import should be relative: testpackage.unsorted
  testpackage/importfromrelative.py:2: direct symbol import foo from testpackage.unsorted
  testpackage/importsymbolfromsub.py:2: direct symbol import nonmodule from testpackage.subpackage
  testpackage/latesymbolimport.py:3: symbol import follows non-symbol import: mercurial.node
  testpackage/multiple.py:2: multiple imported names: os, sys
  testpackage/multiplegroups.py:3: multiple "from . import" statements
  testpackage/relativestdlib.py:2: relative import of stdlib module
  testpackage/requirerelative.py:2: import should be relative: testpackage.unsorted
  testpackage/sortedentries.py:2: imports from testpackage not lexically sorted: bar < foo
  testpackage/stdafterlocal.py:3: stdlib import "os" follows local import: testpackage
  testpackage/subpackage/levelpriority.py:3: higher-level import should come first: testpackage
  testpackage/subpackage/localimport.py:7: multiple "from .. import" statements
  testpackage/subpackage/localimport.py:8: import should be relative: testpackage.subpackage.levelpriority
  testpackage/symbolimport.py:2: direct symbol import foo from testpackage.unsorted
  testpackage/unsorted.py:3: imports not lexically sorted: os < sys
  [1]

  $ cd "$TESTDIR"/..

There are a handful of cases here that require renaming a module so it
doesn't overlap with a stdlib module name. There are also some cycles
here that we should still endeavor to fix, and some cycles will be
hidden by deduplication algorithm in the cycle detector, so fixing
these may expose other cycles.

Known-bad files are excluded by -X as some of them would produce unstable
outputs, which should be fixed later.

  $ hg locate 'mercurial/**.py' 'hgext/**.py' 'tests/**.py' \
  > 'tests/**.t' \
  > -X tests/test-hgweb-auth.py \
  > -X tests/hypothesishelpers.py \
  > -X tests/test-ctxmanager.py \
  > -X tests/test-lock.py \
  > -X tests/test-verify-repo-operations.py \
  > -X tests/test-hook.t \
  > -X tests/test-import.t \
  > -X tests/test-check-module-imports.t \
  > -X tests/test-commit-interactive.t \
  > -X tests/test-contrib-check-code.t \
  > -X tests/test-extension.t \
  > -X tests/test-hghave.t \
  > -X tests/test-hgweb-no-path-info.t \
  > -X tests/test-hgweb-no-request-uri.t \
  > -X tests/test-hgweb-non-interactive.t \
  > | sed 's-\\-/-g' | python "$import_checker" -
  Import cycle: hgext.largefiles.basestore -> hgext.largefiles.localstore -> hgext.largefiles.basestore
  [1]
