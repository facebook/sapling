#require test-repo

  $ . $TESTDIR/require-core-hg.sh contrib/import-checker.py

This file is backported from mercurial/tests/test-check-module-imports.t.
Changes are made to fix paths and remove unnecessary parts.

  $ . "$RUNTESTDIR/helpers-testrepo.sh"
  $ import_checker="$RUNTESTDIR"/../contrib/import-checker.py

  $ cd $TESTDIR/..
  $ hg locate 'set:**.py or grep(r"^#!.*?python")' | sed 's-\\-/-g' | $PYTHON "$import_checker" -
  fastannotate/__init__.py:45: import should be relative: fastannotate
  fastannotate/__init__.py:47: direct symbol import cmdutil, error from mercurial
  fastannotate/__init__.py:47: symbol import follows non-symbol import: mercurial
  fastannotate/__init__.py:71: import should be relative: fastannotate
  fastannotate/commands.py:12: import should be relative: fastannotate
  fastannotate/commands.py:18: direct symbol import commands, error, extensions, patch, scmutil from mercurial
  fastannotate/commands.py:18: symbol import follows non-symbol import: mercurial
  fastannotate/commands.py:26: symbol import follows non-symbol import: mercurial.i18n
  fastannotate/context.py:10: relative import of stdlib module
  fastannotate/context.py:10: direct symbol import defaultdict from collections
  fastannotate/context.py:15: import should be relative: fastannotate
  fastannotate/context.py:15: imports from fastannotate not lexically sorted: error < revmap
  fastannotate/context.py:20: direct symbol import context, error, lock, mdiff, node, scmutil, util from mercurial
  fastannotate/context.py:20: symbol import follows non-symbol import: mercurial
  fastannotate/context.py:29: symbol import follows non-symbol import: mercurial.i18n
  fastannotate/context.py:31: imports not lexically sorted: linelog < os
  fastannotate/hgwebsupport.py:10: direct symbol import extensions, patch from mercurial
  fastannotate/hgwebsupport.py:14: direct symbol import webutil from mercurial.hgweb
  fastannotate/hgwebsupport.py:16: import should be relative: fastannotate
  hgext3rd/absorb.py:30: relative import of stdlib module
  hgext3rd/absorb.py:30: direct symbol import defaultdict from collections
  hgext3rd/absorb.py:33: direct symbol import cmdutil, commands, context, crecord, error, extensions, mdiff, node, obsolete, patch, phases, repair, scmutil, util from mercurial
  hgext3rd/mergedriver.py:15: direct symbol import commands, error, extensions, hook, merge, util from mercurial
  hgext3rd/smartlog.py:26: relative import of stdlib module
  hgext3rd/smartlog.py:26: direct symbol import chain from itertools
  hgext3rd/smartlog.py:29: direct symbol import bookmarks, cmdutil, commands, error, extensions, graphmod, obsolete, phases, revset, scmutil, templatekw, util from mercurial
  hgext3rd/smartlog.py:43: direct symbol import node from mercurial
  hgext3rd/smartlog.py:45: direct symbol import pager from hgext
  infinitepush/__init__.py:15: direct symbol import bundle2, changegroup, cmdutil, commands, discovery, encoding, error, exchange, extensions, hg, localrepo, util, pushkey, revset, phases, wireproto from mercurial
  infinitepush/__init__.py:15: imports from mercurial not lexically sorted: pushkey < util
  infinitepush/__init__.py:15: imports from mercurial not lexically sorted: phases < revset
  infinitepush/__init__.py:34: direct symbol import wrapcommand, wrapfunction from mercurial.extensions
  infinitepush/__init__.py:35: direct symbol import repository from mercurial.hg
  infinitepush/__init__.py:38: direct symbol import batchable, future from mercurial.peer
  infinitepush/__init__.py:39: direct symbol import encodelist, decodelist from mercurial.wireproto
  infinitepush/__init__.py:39: imports from mercurial.wireproto not lexically sorted: decodelist < encodelist
  infinitepush/tests/testindex.py:3: direct symbol import hg, ui from mercurial
  infinitepush/tests/testindex.py:3: ui from mercurial must be "as" aliased to uimod
  infinitepush/tests/testindex.py:4: direct symbol import getrandomid, getfileindexandrepo from infinitepush.tests.util
  infinitepush/tests/testindex.py:4: imports from infinitepush.tests.util not lexically sorted: getfileindexandrepo < getrandomid
  infinitepush/tests/teststore.py:2: direct symbol import getrepo, getfilebundlestore, getrandomid from infinitepush.tests.util
  infinitepush/tests/teststore.py:2: imports from infinitepush.tests.util not lexically sorted: getfilebundlestore < getrepo
  infinitepush/tests/teststore.py:3: direct symbol import ui from mercurial
  infinitepush/tests/teststore.py:3: ui from mercurial must be "as" aliased to uimod
  remotefilelog/cacheclient.py:14: mixed imports
     stdlib:    os, sys
     relative:  memcache
  remotefilelog/shallowrepo.py:12: mixed imports
     stdlib:    os
     relative:  fileserverclient, remotefilectx, remotefilelog, shallowbundle
  tests/test-fastannotate-revmap.py:3: multiple imported names: os, sys, tempfile
  Import cycle: fastmanifest.cachemanager -> fastmanifest.implementation -> fastmanifest.cachemanager
  Import cycle: remotefilelog.fileserverclient -> remotefilelog.shallowrepo -> remotefilelog.fileserverclient
  [1]
