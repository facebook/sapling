#require test-repo

  $ . $TESTDIR/require-core-hg.sh contrib/import-checker.py

This file is backported from mercurial/tests/test-check-module-imports.t.
Changes are made to fix paths and remove unnecessary parts.

We ignore "direct symbol import ... from mercurial/hgext", and "symbol
import follows non-symbol import: mercurial" errors, as they are valid
use-cases, and there is no clean way to tell the checker to read mecurial
modules, or change the whitelist (allowsymbolimports).

  $ . "$RUNTESTDIR/helpers-testrepo.sh"
  $ import_checker="$RUNTESTDIR"/../contrib/import-checker.py

  $ cd $TESTDIR/..
  $ hg locate 'set:**.py or grep(r"^#!.*?python")' | sed 's-\\-/-g' | $PYTHON "$import_checker" - \
  > | egrep -v 'symbol import .* (mercurial|hgext)$'
  fastannotate/__init__.py:51: relative import of stdlib module
  fastannotate/__init__.py:72: relative import of stdlib module
  fastannotate/commands.py:21: relative import of stdlib module
  fastannotate/context.py:26: relative import of stdlib module
  fastannotate/context.py:31: imports not lexically sorted: linelog < os
  fastannotate/context.py:31: stdlib import "linelog" follows local import: mercurial.i18n
  fastannotate/hgwebsupport.py:16: relative import of stdlib module
  hgext3rd/smartlog.py:26: relative import of stdlib module
  hgext3rd/smartlog.py:26: direct symbol import chain from itertools
  infinitepush/__init__.py:15: imports from mercurial not lexically sorted: pushkey < util
  infinitepush/__init__.py:15: imports from mercurial not lexically sorted: phases < revset
  infinitepush/__init__.py:34: direct symbol import wrapcommand, wrapfunction from mercurial.extensions
  infinitepush/__init__.py:35: direct symbol import repository from mercurial.hg
  infinitepush/__init__.py:38: direct symbol import batchable, future from mercurial.peer
  infinitepush/__init__.py:39: direct symbol import encodelist, decodelist from mercurial.wireproto
  infinitepush/__init__.py:39: imports from mercurial.wireproto not lexically sorted: decodelist < encodelist
  infinitepush/__init__.py:64: relative import of stdlib module
  infinitepush/__init__.py:77: relative import of stdlib module
  infinitepush/__init__.py:94: relative import of stdlib module
  infinitepush/__init__.py:104: relative import of stdlib module
  infinitepush/tests/testindex.py:3: ui from mercurial must be "as" aliased to uimod
  infinitepush/tests/testindex.py:4: direct symbol import getrandomid, getfileindexandrepo from infinitepush.tests.util
  infinitepush/tests/testindex.py:4: imports from infinitepush.tests.util not lexically sorted: getfileindexandrepo < getrandomid
  infinitepush/tests/testindex.py:5: relative import of stdlib module
  infinitepush/tests/teststore.py:2: direct symbol import getrepo, getfilebundlestore, getrandomid from infinitepush.tests.util
  infinitepush/tests/teststore.py:2: imports from infinitepush.tests.util not lexically sorted: getfilebundlestore < getrepo
  infinitepush/tests/teststore.py:3: ui from mercurial must be "as" aliased to uimod
  infinitepush/tests/teststore.py:4: relative import of stdlib module
  remotefilelog/__init__.py:8: mixed imports
     stdlib:    remotefilelog
     relative:  fileserverclient, remotefilectx, shallowrepo, shallowstore
  remotefilelog/cacheclient.py:14: mixed imports
     stdlib:    os, sys
     relative:  memcache
  remotefilelog/datapack.py:6: mixed imports
     stdlib:    cdatapack
     relative:  basepack, constants, shallowutil
  remotefilelog/shallowbundle.py:8: mixed imports
     stdlib:    remotefilelog
     relative:  fileserverclient, shallowutil
  remotefilelog/shallowrepo.py:12: mixed imports
     stdlib:    os, remotefilelog
     relative:  fileserverclient, remotefilectx, shallowbundle
  tests/test-fastannotate-revmap.py:10: relative import of stdlib module
  Import cycle: fastmanifest.cachemanager -> fastmanifest.implementation -> fastmanifest.cachemanager
  Import cycle: remotefilelog.fileserverclient -> remotefilelog.shallowrepo -> remotefilelog.fileserverclient
