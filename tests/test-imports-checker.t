#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ testrepohgenv
  $ import_checker="$TESTDIR"/../contrib/import-checker.py

Run the doctests from the import checker, and make sure
it's working correctly.
  $ TERM=dumb
  $ export TERM
  $ $PYTHON -m doctest $import_checker

Run additional tests for the import checker

  $ mkdir testpackage
  $ touch testpackage/__init__.py

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

  $ mkdir testpackage2
  $ touch testpackage2/__init__.py

  $ cat > testpackage2/latesymbolimport.py << EOF
  > from __future__ import absolute_import
  > from testpackage import unsorted
  > from mercurial.node import hex
  > EOF

# Shadowing a stdlib module to test "relative import of stdlib module" is
# allowed if the module is also being checked

  $ mkdir email
  $ touch email/__init__.py
  $ touch email/errors.py
  $ cat > email/utils.py << EOF
  > from __future__ import absolute_import
  > from . import errors
  > EOF

  $ $PYTHON "$import_checker" testpackage*/*.py testpackage/subpackage/*.py \
  >   email/*.py
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
  testpackage2/latesymbolimport.py:3: symbol import follows non-symbol import: mercurial.node
  [1]
