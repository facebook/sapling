#require test-repo pyflakes hg10

  $ . $TESTDIR/require-core-hg.sh tests/filterpyflakes.py

This file is backported from mercurial/tests/test-check-pyflakes.t.
It differs slightly to fix paths.

  $ . "$RUNTESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

run pyflakes on all tracked files ending in .py or without a file ending
(skipping binary file random-seed)

  $ hg locate 'set:**.py or grep("^#!.*python")' > "$TESTTMP/files1"
  $ if [ -n "$LINTFILES" ]; then
  >   printf "$LINTFILES" > "$TESTTMP/files2"
  >   join "$TESTTMP/files1" "$TESTTMP/files2"
  > else
  >   cat "$TESTTMP/files1"
  > fi \
  > | xargs pyflakes 2>/dev/null | "$RUNTESTDIR/filterpyflakes.py"
  fastmanifest/cachemanager.py:10: 'sys' imported but unused
  fastmanifest/concurrency.py:9: 'resource' imported but unused
  hgext3rd/arcdiff.py:9: 'cmdutil' imported but unused
  hgext3rd/arcdiff.py:9: 'hg' imported but unused
  hgext3rd/arcdiff.py:9: 'scmutil' imported but unused
  hgext3rd/arcdiff.py:9: 'util' imported but unused
  hgext3rd/arcdiff.py:12: 'json' imported but unused
  hgext3rd/arcdiff.py:13: 'os' imported but unused
  hgext3rd/arcdiff.py:14: 're' imported but unused
  hgext3rd/arcdiff.py:15: 'subprocess' imported but unused
  hgext3rd/backups.py:9: 'extensions' imported but unused
  hgext3rd/backups.py:10: 'changegroup' imported but unused
  hgext3rd/catnotate.py:1: 'commands' imported but unused
  hgext3rd/catnotate.py:1: 'error' imported but unused
  hgext3rd/catnotate.py:1: 'extensions' imported but unused
  hgext3rd/catnotate.py:3: 'matchmod' imported but unused
  hgext3rd/commitextras.py:8: 'hg' imported but unused
  hgext3rd/commitextras.py:8: 'scmutil' imported but unused
  hgext3rd/commitextras.py:8: 'util' imported but unused
  hgext3rd/commitextras.py:9: 'bookmarks' imported but unused
  hgext3rd/commitextras.py:11: 'rebase' imported but unused
  hgext3rd/commitextras.py:12: 'errno' imported but unused
  hgext3rd/commitextras.py:12: 'os' imported but unused
  hgext3rd/commitextras.py:12: 'stat' imported but unused
  hgext3rd/commitextras.py:12: 'subprocess' imported but unused
  hgext3rd/dirsync.py:35: 'commands' imported but unused
  hgext3rd/errorredirect.py:35: 'sys' imported but unused
  hgext3rd/fbamend.py:35: 'chain' imported but unused
  hgext3rd/fbhistedit.py:22: 'util' imported but unused
  hgext3rd/githelp.py:15: 'commands' imported but unused
  hgext3rd/githelp.py:15: 'extensions' imported but unused
  hgext3rd/githelp.py:16: 'changegroup' imported but unused
  hgext3rd/githelp.py:16: 'hg' imported but unused
  hgext3rd/githelp.py:17: 'wrapfunction' imported but unused
  hgext3rd/githelp.py:19: 'hex' imported but unused
  hgext3rd/githelp.py:19: 'nullid' imported but unused
  hgext3rd/githelp.py:19: 'nullrev' imported but unused
  hgext3rd/githelp.py:21: 'errno' imported but unused
  hgext3rd/githelp.py:21: 'glob' imported but unused
  hgext3rd/githelp.py:21: 'os' imported but unused
  hgext3rd/gitlookup.py:24: 'extensions' imported but unused
  hgext3rd/inhibitwarn.py:15: 'localrepo' imported but unused
  hgext3rd/logginghelper.py:17: '_' imported but unused
  hgext3rd/mergedriver.py:15: 'util' imported but unused
  hgext3rd/perftweaks.py:9: 'util' imported but unused
  hgext3rd/perftweaks.py:10: 'wrapcommand' imported but unused
  hgext3rd/perftweaks.py:11: '_' imported but unused
  hgext3rd/phabstatus.py:13: 're' imported but unused
  hgext3rd/phabstatus.py:14: 'subprocess' imported but unused
  hgext3rd/phabstatus.py:15: 'os' imported but unused
  hgext3rd/phabstatus.py:16: 'json' imported but unused
  hgext3rd/phrevset.py:29: 'hgutil' imported but unused
  hgext3rd/profiling.py:20: 'signal' imported but unused
  hgext3rd/profiling.py:21: 'util' imported but unused
  hgext3rd/pullcreatemarkers.py:15: 're' imported but unused
  hgext3rd/rage.py:6: 'extensions' imported but unused
  hgext3rd/rage.py:6: 'ui' imported but unused
  hgext3rd/rage.py:8: 'blackbox' imported but unused
  hgext3rd/reset.py:6: 'short' imported but unused
  hgext3rd/reset.py:8: 'util' imported but unused
  hgext3rd/reset.py:11: 'struct' imported but unused
  hgext3rd/sparse.py:17: 'errno' imported but unused
  hgext3rd/sparse.py:17: 're' imported but unused
  hgext3rd/sshaskpass.py:25: 'errno' imported but unused
  hgext3rd/tweakdefaults.py:29: 'errno' imported but unused
  infinitepush/__init__.py:36: 'bin' imported but unused
  infinitepush/fileindexapi.py:3: 'indexexception' imported but unused
  infinitepush/store.py:10: 'util' imported but unused
  infinitepush/tests/util.py:6: 'time' imported but unused
  infinitepush/tests/util.py:7: 'getpass' imported but unused
  infinitepush/tests/util.py:9: 'subprocess' imported but unused
  phabricator/conduit.py:12: 'sys' imported but unused
  remotefilelog/__init__.py:10: 'basestore' imported but unused
  remotefilelog/__init__.py:10: 'contentstore' imported but unused
  remotefilelog/__init__.py:10: 'datapack' imported but unused
  remotefilelog/__init__.py:10: 'historypack' imported but unused
  remotefilelog/__init__.py:11: 'metadatastore' imported but unused
  remotefilelog/__init__.py:13: 'bin' imported but unused
  remotefilelog/__init__.py:13: 'nullid' imported but unused
  remotefilelog/__init__.py:13: 'nullrev' imported but unused
  remotefilelog/__init__.py:13: 'short' imported but unused
  remotefilelog/__init__.py:16: 'ancestor' imported but unused
  remotefilelog/__init__.py:16: 'dagutil' imported but unused
  remotefilelog/__init__.py:16: 'mdiff' imported but unused
  remotefilelog/__init__.py:16: 'parsers' imported but unused
  remotefilelog/__init__.py:17: 'filelog' imported but unused
  remotefilelog/__init__.py:17: 'wireproto' imported but unused
  remotefilelog/__init__.py:19: 'sshpeer' imported but unused
  remotefilelog/__init__.py:20: 'verify' imported but unused
  remotefilelog/__init__.py:22: 'collections' imported but unused
  remotefilelog/__init__.py:22: 'errno' imported but unused
  remotefilelog/__init__.py:22: 'lz4' imported but unused
  remotefilelog/__init__.py:22: 'socket' imported but unused
  remotefilelog/__init__.py:22: 'struct' imported but unused
  remotefilelog/__init__.py:22: 'subprocess' imported but unused
  remotefilelog/__init__.py:22: 'time' imported but unused
  remotefilelog/__init__.py:22: 'zlib' imported but unused
  remotefilelog/__init__.py:23: 'stat' imported but unused
  remotefilelog/__init__.py:24: 'sys' imported but unused
  remotefilelog/basepack.py:4: 'constants' imported but unused
  remotefilelog/contentstore.py:1: 'os' imported but unused
  remotefilelog/contentstore.py:1: 'shutil' imported but unused
  remotefilelog/contentstore.py:3: 'error' imported but unused
  remotefilelog/contentstore.py:4: '_' imported but unused
  remotefilelog/datapack.py:1: 'errno' imported but unused
  remotefilelog/datapack.py:1: 'mmap' imported but unused
  remotefilelog/datapack.py:1: 'os' imported but unused
  remotefilelog/datapack.py:1: 'tempfile' imported but unused
  remotefilelog/datapack.py:2: 'defaultdict' imported but unused
  remotefilelog/datapack.py:3: 'mdiff' imported but unused
  remotefilelog/datapack.py:3: 'osutil' imported but unused
  remotefilelog/datapack.py:4: 'bin' imported but unused
  remotefilelog/datapack.py:6: 'shallowutil' imported but unused
  remotefilelog/debugcommands.py:9: 'nullrev' imported but unused
  remotefilelog/fileserverclient.py:10: 'node' imported but unused
  remotefilelog/fileserverclient.py:12: 'socket' imported but unused
  remotefilelog/fileserverclient.py:13: 'errno' imported but unused
  remotefilelog/fileserverclient.py:20: 'grp' imported but unused
  remotefilelog/historypack.py:1: 'errno' imported but unused
  remotefilelog/historypack.py:1: 'lz4' imported but unused
  remotefilelog/historypack.py:1: 'mmap' imported but unused
  remotefilelog/historypack.py:1: 'os' imported but unused
  remotefilelog/historypack.py:1: 'tempfile' imported but unused
  remotefilelog/historypack.py:2: 'defaultdict' imported but unused
  remotefilelog/historypack.py:2: 'deque' imported but unused
  remotefilelog/historypack.py:3: 'mdiff' imported but unused
  remotefilelog/historypack.py:3: 'osutil' imported but unused
  remotefilelog/historypack.py:4: 'bin' imported but unused
  remotefilelog/historypack.py:4: 'nullid' imported but unused
  remotefilelog/historypack.py:5: '_' imported but unused
  remotefilelog/historypack.py:6: 'shallowutil' imported but unused
  remotefilelog/metadatastore.py:1: 'os' imported but unused
  remotefilelog/metadatastore.py:3: 'error' imported but unused
  remotefilelog/metadatastore.py:3: 'util' imported but unused
  remotefilelog/metadatastore.py:4: '_' imported but unused
  remotefilelog/remotefilectx.py:8: 'os' imported but unused
  remotefilelog/remotefilectx.py:9: 'short' imported but unused
  remotefilelog/remotefilectx.py:10: 'filelog' imported but unused
  remotefilelog/remotefilectx.py:10: 'mdiff' imported but unused
  remotefilelog/remotefilectx.py:10: 'revlog' imported but unused
  remotefilelog/remotefilelog.py:10: 'errno' imported but unused
  remotefilelog/remotefilelog.py:10: 'shutil' imported but unused
  remotefilelog/remotefilelog.py:11: 'hex' imported but unused
  remotefilelog/remotefilelog.py:11: 'nullrev' imported but unused
  remotefilelog/remotefilelog.py:12: 'filelog' imported but unused
  remotefilelog/remotefilelog.py:12: 'util' imported but unused
  remotefilelog/remotefilelogserver.py:14: 'shallowutil' imported but unused
  remotefilelog/remotefilelogserver.py:15: 'sys' imported but unused
  remotefilelog/repack.py:4: 'bin' imported but unused
  remotefilelog/repack.py:4: 'hex' imported but unused
  remotefilelog/shallowbundle.py:9: 'collections' imported but unused
  remotefilelog/shallowbundle.py:11: 'phases' imported but unused
  remotefilelog/shallowrepo.py:8: 'bin' imported but unused
  remotefilelog/shallowrepo.py:9: '_' imported but unused
  remotefilelog/shallowrepo.py:10: 'context' imported but unused
  remotefilelog/shallowrepo.py:11: 'wrapfunction' imported but unused
  remotefilelog/shallowrepo.py:12: 'shallowbundle' imported but unused
  sqldirstate/__init__.py:10: 'DBFILE' imported but unused
  tests/perftest.py:10: 'pdb' imported but unused
  tests/test-remotefilelog-datapack.py:1: 'binascii' imported but unused
  tests/test-remotefilelog-datapack.py:3: 'itertools' imported but unused
  tests/test-remotefilelog-datapack.py:13: 'datapackstore' imported but unused
  tests/test-remotefilelog-datapack.py:26: 'bin' imported but unused
  tests/test-remotefilelog-datapack.py:26: 'hex' imported but unused
  tests/test-remotefilelog-histpack.py:1: 'binascii' imported but unused
  tests/test-remotefilelog-histpack.py:3: 'itertools' imported but unused
  tests/test-remotefilelog-histpack.py:13: 'historypackstore' imported but unused
  tests/test-remotefilelog-histpack.py:16: 'bin' imported but unused
  tests/test-remotefilelog-histpack.py:16: 'hex' imported but unused
  tests/test-remotefilelog-histpack.py:19: 'SMALLFANOUTPREFIX' imported but unused
  tests/treemanifest_correctness.py:8: 'error' imported but unused
  tests/treemanifest_correctness.py:11: 'pdb' imported but unused
  tests/treemanifest_correctness.py:12: 'fastmanifestcache' imported but unused
  treemanifest/__init__.py:17: 'hex' imported but unused
  treemanifest/__init__.py:17: 'nullrev' imported but unused
  fastmanifest/cachemanager.py:212: local variable 'lock' is assigned to but never used
  fastmanifest/implementation.py:643: local variable 'ret' is assigned to but never used
  hgext3rd/catnotate.py:26: local variable 'files' is assigned to but never used
  hgext3rd/fastlog.py:358: local variable 'queue' is assigned to but never used
  hgext3rd/fbamend.py:148: local variable 'currev' is assigned to but never used
  hgext3rd/fbamend.py:158: local variable 'precrev' is assigned to but never used
  hgext3rd/fbconduit.py:191: local variable 'peerpath' is assigned to but never used
  hgext3rd/fbconduit.py:205: local variable 'e' is assigned to but never used
  hgext3rd/fbhistedit.py:145: local variable 'histedit' is assigned to but never used
  hgext3rd/grepdiff.py:74: local variable 'res' is assigned to but never used
  hgext3rd/pullcreatemarkers.py:62: local variable 'l' is assigned to but never used
  hgext3rd/pullcreatemarkers.py:63: local variable 't' is assigned to but never used
  hgext3rd/pushvars.py:33: local variable 'e' is assigned to but never used
  hgext3rd/smartlog.py:226: local variable 'fakes' is assigned to but never used
  hgext3rd/sparse.py:766: local variable 'wctx' is assigned to but never used
  hgext3rd/sshaskpass.py:87: local variable 'ppid' is assigned to but never used
  hgext3rd/sshaskpass.py:151: local variable 'parentpids' is assigned to but never used
  hgext3rd/tweakdefaults.py:208: local variable 'rebasehint' is assigned to but never used
  hgext3rd/uncommit.py:146: local variable 'wm' is assigned to but never used
  remotefilelog/__init__.py:777: local variable 'm' is assigned to but never used
  remotefilelog/debugcommands.py:169: local variable 'data' is assigned to but never used
  remotefilelog/fileserverclient.py:555: local variable 'reponame' is assigned to but never used
  remotefilelog/historypack.py:39: local variable 'ex' is assigned to but never used
  remotefilelog/historypack.py:63: local variable 'value' is assigned to but never used
  remotefilelog/historypack.py:156: local variable 'index' is assigned to but never used
  remotefilelog/historypack.py:194: local variable 'revcount' is assigned to but never used
  remotefilelog/remotefilectx.py:229: local variable 'clrev' is assigned to but never used
  remotefilelog/remotefilelogserver.py:277: local variable 'opener' is assigned to but never used
  remotefilelog/shallowbundle.py:321: local variable 'copyfl' is assigned to but never used
  tests/perftest.py:232: local variable 'fakestore' is assigned to but never used
  tests/test-remotefilelog-ctreemanifest.py:51: local variable 'a' is assigned to but never used
  tests/test-remotefilelog-datapack.py:264: local variable 'result' is assigned to but never used
  fastmanifest/implementation.py:18: 'from constants import *' used; unable to detect undefined names
  hgext3rd/fbconduit.py:187: undefined name 'false'
  hgext3rd/gitlookup.py:130: undefined name 'ui'
  hgext3rd/grepdiff.py:36: undefined name 'repo'
  hgext3rd/mergedriver.py:156: undefined name 'origcls'
  hgext3rd/statprofext.py:28: undefined name '_'
  hgext3rd/statprofext.py:28: undefined name 'error'
  hgext3rd/statprofext.py:35: undefined name '_'
  hgext3rd/statprofext.py:51: undefined name '_'
  hgext3rd/upgradegeneraldelta.py:63: undefined name '_'
  hgext3rd/upgradegeneraldelta.py:77: undefined name '_'
  hgext3rd/upgradegeneraldelta.py:109: undefined name '_'
  remotefilelog/debugcommands.py:87: undefined name 'error'
  remotefilelog/debugcommands.py:151: undefined name 'fileserverclient'
  remotefilelog/debugcommands.py:151: undefined name 'repo'
  tests/perftest.py:165: undefined name 'mdiff'
  tests/treemanifest_correctness.py:163: undefined name 'mdiff'
  treemanifest/__init__.py:141: undefined name 'mpatchError'
  
