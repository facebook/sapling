#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ import_checker="$TESTDIR"/../contrib/import-checker.py

  $ cd "$TESTDIR"/..

There are a handful of cases here that require renaming a module so it
doesn't overlap with a stdlib module name. There are also some cycles
here that we should still endeavor to fix, and some cycles will be
hidden by deduplication algorithm in the cycle detector, so fixing
these may expose other cycles.

Known-bad files are excluded by -X as some of them would produce unstable
outputs, which should be fixed later.

  $ testrepohg locate 'set:**.py or grep(r"^#!.*?python")' \
  > 'tests/**.t' \
  > -X hgweb.cgi \
  > -X setup.py \
  > -X contrib/debugshell.py \
  > -X contrib/hgweb.fcgi \
  > -X contrib/python-zstandard/ \
  > -X contrib/win32/hgwebdir_wsgi.py \
  > -X doc/gendoc.py \
  > -X doc/hgmanpage.py \
  > -X i18n/posplit \
  > -X mercurial/thirdparty \
  > -X tests/hypothesishelpers.py \
  > -X tests/test-commit-interactive.t \
  > -X tests/test-contrib-check-code.t \
  > -X tests/test-demandimport.py \
  > -X tests/test-extension.t \
  > -X tests/test-hghave.t \
  > -X tests/test-hgweb-auth.py \
  > -X tests/test-hgweb-no-path-info.t \
  > -X tests/test-hgweb-no-request-uri.t \
  > -X tests/test-hgweb-non-interactive.t \
  > -X tests/test-hook.t \
  > -X tests/test-import.t \
  > -X tests/test-imports-checker.t \
  > -X tests/test-lock.py \
  > -X tests/test-verify-repo-operations.py \
  > | sed 's-\\-/-g' | $PYTHON "$import_checker" -
  fb-hgext/contrib/git-sl:13: relative import of stdlib module
  fb-hgext/contrib/git-sl:13: direct symbol import Popen from subprocess
  fb-hgext/contrib/git-sl:15: relative import of stdlib module
  fb-hgext/contrib/git-sl:15: direct symbol import time from time
  fb-hgext/contrib/git-sl:16: imports not lexically sorted: argparse < subprocess
  fb-hgext/distutils_rust/__init__.py:6: relative import of stdlib module
  fb-hgext/distutils_rust/__init__.py:6: direct symbol import Command from distutils.core
  fb-hgext/distutils_rust/__init__.py:7: relative import of stdlib module
  fb-hgext/distutils_rust/__init__.py:7: direct symbol import Distribution from distutils.dist
  fb-hgext/distutils_rust/__init__.py:8: relative import of stdlib module
  fb-hgext/distutils_rust/__init__.py:8: direct symbol import CompileError from distutils.errors
  fb-hgext/distutils_rust/__init__.py:9: relative import of stdlib module
  fb-hgext/distutils_rust/__init__.py:9: direct symbol import log from distutils
  fb-hgext/distutils_rust/__init__.py:10: relative import of stdlib module
  fb-hgext/distutils_rust/__init__.py:10: direct symbol import build from distutils.command.build
  fb-hgext/infinitepush/__init__.py:102: direct symbol import copiedpart, getscratchbranchparts, scratchbookmarksparttype, scratchbranchparttype from fb-hgext.infinitepush.bundleparts
  fb-hgext/infinitepush/__init__.py:108: imports from fb-hgext.infinitepush not lexically sorted: common < infinitepushcommands
  fb-hgext/infinitepush/__init__.py:113: relative import of stdlib module
  fb-hgext/infinitepush/__init__.py:113: direct symbol import defaultdict from collections
  fb-hgext/infinitepush/__init__.py:113: symbol import follows non-symbol import: collections
  fb-hgext/infinitepush/__init__.py:114: relative import of stdlib module
  fb-hgext/infinitepush/__init__.py:114: direct symbol import partial from functools
  fb-hgext/infinitepush/__init__.py:114: symbol import follows non-symbol import: functools
  fb-hgext/infinitepush/__init__.py:132: direct symbol import wrapcommand, wrapfunction, unwrapfunction from mercurial.extensions
  fb-hgext/infinitepush/__init__.py:132: symbol import follows non-symbol import: mercurial.extensions
  fb-hgext/infinitepush/__init__.py:132: imports from mercurial.extensions not lexically sorted: unwrapfunction < wrapfunction
  fb-hgext/infinitepush/__init__.py:133: direct symbol import repository from mercurial.hg
  fb-hgext/infinitepush/__init__.py:133: symbol import follows non-symbol import: mercurial.hg
  fb-hgext/infinitepush/__init__.py:134: symbol import follows non-symbol import: mercurial.node
  fb-hgext/infinitepush/__init__.py:135: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/infinitepush/__init__.py:136: direct symbol import batchable, future from mercurial.peer
  fb-hgext/infinitepush/__init__.py:136: symbol import follows non-symbol import: mercurial.peer
  fb-hgext/infinitepush/__init__.py:137: direct symbol import encodelist, decodelist from mercurial.wireproto
  fb-hgext/infinitepush/__init__.py:137: symbol import follows non-symbol import: mercurial.wireproto
  fb-hgext/infinitepush/__init__.py:137: imports from mercurial.wireproto not lexically sorted: decodelist < encodelist
  fb-hgext/infinitepush/backupcommands.py:52: direct symbol import getscratchbookmarkspart, getscratchbranchparts from fb-hgext.infinitepush.bundleparts
  fb-hgext/infinitepush/backupcommands.py:74: relative import of stdlib module
  fb-hgext/infinitepush/backupcommands.py:74: direct symbol import defaultdict, namedtuple from collections
  fb-hgext/infinitepush/backupcommands.py:74: symbol import follows non-symbol import: collections
  fb-hgext/infinitepush/backupcommands.py:76: direct symbol import wrapfunction, unwrapfunction from mercurial.extensions
  fb-hgext/infinitepush/backupcommands.py:76: symbol import follows non-symbol import: mercurial.extensions
  fb-hgext/infinitepush/backupcommands.py:76: imports from mercurial.extensions not lexically sorted: unwrapfunction < wrapfunction
  fb-hgext/infinitepush/backupcommands.py:77: symbol import follows non-symbol import: mercurial.node
  fb-hgext/infinitepush/backupcommands.py:78: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/infinitepush/backupcommands.py:80: direct symbol import shareutil from hgext3rd
  fb-hgext/infinitepush/backupcommands.py:80: symbol import follows non-symbol import: hgext3rd
  fb-hgext/infinitepush/infinitepushcommands.py:18: direct symbol import cmdtable from fb-hgext.infinitepush.backupcommands
  fb-hgext/infinitepush/infinitepushcommands.py:31: direct symbol import downloadbundle from fb-hgext.infinitepush.common
  fb-hgext/infinitepush/infinitepushcommands.py:31: symbol import follows non-symbol import: fb-hgext.infinitepush.common
  fb-hgext/infinitepush/infinitepushcommands.py:32: symbol import follows non-symbol import: mercurial.node
  fb-hgext/infinitepush/infinitepushcommands.py:33: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/scripts/traceprof.py:5: direct symbol import traceprof from hgext3rd
  fb-hgext/scripts/traceprof.py:6: ui from mercurial must be "as" aliased to uimod
  fb-hgext/scripts/traceprof.py:8: stdlib import "os" follows local import: mercurial
  fb-hgext/scripts/traceprof.py:9: stdlib import "sys" follows local import: mercurial
  fb-hgext/tests/check-ext.py:4: relative import of stdlib module
  fb-hgext/tests/check-ext.py:4: direct symbol import glob from glob
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:17: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:20: relative import of stdlib module
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:20: direct symbol import datapackstore from cstore
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:24: relative import of stdlib module
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:24: direct symbol import fastdatapack, mutabledatapack from remotefilelog.datapack
  fb-hgext/tests/test-fb-hgext-cstore-datapackstore.py:30: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:16: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:19: relative import of stdlib module
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:19: direct symbol import datapackstore, uniondatapackstore from cstore
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:24: relative import of stdlib module
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:24: direct symbol import datapack, mutabledatapack from remotefilelog.datapack
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:30: symbol import follows non-symbol import: mercurial.node
  fb-hgext/tests/test-fb-hgext-cstore-uniondatapackstore.py:31: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-fb-hgext-patchrmdir.py:13: direct symbol import patchrmdir from hgext3rd
  hgext/age.py:21: imports not lexically sorted: re < time
  hgext/cleanobsstore.py:37: symbol import follows non-symbol import: mercurial.i18n
  hgext/conflictinfo.py:30: direct symbol import absentfilectx from mercurial.filemerge
  hgext/crdump.py:5: multiple imported names: json, re, shutil, tempfile
  hgext/crdump.py:6: relative import of stdlib module
  hgext/crdump.py:6: direct symbol import path from os
  hgext/crdump.py:16: symbol import follows non-symbol import: mercurial.i18n
  hgext/crdump.py:17: symbol import follows non-symbol import: mercurial.node
  hgext/dirsync.py:46: symbol import follows non-symbol import: mercurial.i18n
  hgext/fastannotate/context.py:23: symbol import follows non-symbol import: mercurial.i18n
  hgext/fastannotate/context.py:30: imports not lexically sorted: linelog < os
  hgext/fastannotate/context.py:30: stdlib import "linelog" follows local import: hgext.fastannotate
  hgext/fastannotate/revmap.py:16: symbol import follows non-symbol import: mercurial.node
  hgext/fastverify.py:24: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/__init__.py:58: symbol import follows non-symbol import: mercurial.node
  hgext/fbamend/__init__.py:60: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/__init__.py:62: import should be relative: hgext
  hgext/fbamend/__init__.py:81: stdlib import "tempfile" follows local import: hgext.fbamend
  hgext/fbamend/common.py:10: relative import of stdlib module
  hgext/fbamend/common.py:10: direct symbol import defaultdict from collections
  hgext/fbamend/common.py:12: import should be relative: hgext
  hgext/fbamend/common.py:21: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/common.py:22: symbol import follows non-symbol import: mercurial.node
  hgext/fbamend/fold.py:23: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/hiddenoverride.py:10: direct symbol import extutil from hgext3rd
  hgext/fbamend/hide.py:12: imports from mercurial not lexically sorted: extensions < hg
  hgext/fbamend/hide.py:22: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/metaedit.py:23: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/movement.py:10: relative import of stdlib module
  hgext/fbamend/movement.py:10: direct symbol import count from itertools
  hgext/fbamend/movement.py:21: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/movement.py:22: symbol import follows non-symbol import: mercurial.node
  hgext/fbamend/prune.py:14: imports from mercurial not lexically sorted: repair < util
  hgext/fbamend/prune.py:26: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/restack.py:16: import should be relative: hgext
  hgext/fbamend/split.py:25: symbol import follows non-symbol import: mercurial.i18n
  hgext/fbamend/unamend.py:17: symbol import follows non-symbol import: mercurial.i18n
  hgext/gitrevset.py:25: symbol import follows non-symbol import: mercurial.i18n
  hgext/gitrevset.py:26: stdlib import "re" follows local import: mercurial.i18n
  hgext/hiddenerror.py:28: symbol import follows non-symbol import: mercurial.i18n
  hgext/hiddenerror.py:29: symbol import follows non-symbol import: mercurial.node
  hgext/obsshelve.py:56: import should be relative: hgext
  hgext/p4fastimport/__init__.py:35: imports from hgext.p4fastimport not lexically sorted: importer < p4
  hgext/p4fastimport/__init__.py:35: imports from hgext.p4fastimport not lexically sorted: filetransaction < importer
  hgext/p4fastimport/__init__.py:41: direct symbol import runworker, lastcl, decodefileflags from hgext.p4fastimport.util
  hgext/p4fastimport/__init__.py:41: symbol import follows non-symbol import: hgext.p4fastimport.util
  hgext/p4fastimport/__init__.py:41: imports from hgext.p4fastimport.util not lexically sorted: lastcl < runworker
  hgext/p4fastimport/__init__.py:41: imports from hgext.p4fastimport.util not lexically sorted: decodefileflags < lastcl
  hgext/p4fastimport/__init__.py:43: symbol import follows non-symbol import: mercurial.i18n
  hgext/p4fastimport/__init__.py:44: symbol import follows non-symbol import: mercurial.node
  hgext/p4fastimport/__init__.py:44: imports from mercurial.node not lexically sorted: hex < short
  hgext/p4fastimport/importer.py:19: direct symbol import caseconflict, localpath from hgext.p4fastimport.util
  hgext/p4fastimport/importer.py:19: symbol import follows non-symbol import: hgext.p4fastimport.util
  hgext/p4fastimport/p4.py:11: direct symbol import runworker from hgext.p4fastimport.util
  hgext/pushrebase.py:27: multiple imported names: errno, os, tempfile, mmap, time
  hgext/pushrebase.py:49: direct symbol import wrapcommand, wrapfunction, unwrapfunction from mercurial.extensions
  hgext/pushrebase.py:49: symbol import follows non-symbol import: mercurial.extensions
  hgext/pushrebase.py:49: imports from mercurial.extensions not lexically sorted: unwrapfunction < wrapfunction
  hgext/pushrebase.py:50: symbol import follows non-symbol import: mercurial.node
  hgext/pushrebase.py:50: imports from mercurial.node not lexically sorted: hex < nullid
  hgext/pushrebase.py:50: imports from mercurial.node not lexically sorted: bin < hex
  hgext/pushrebase.py:51: symbol import follows non-symbol import: mercurial.i18n
  hgext/pushrebase.py:53: relative import of stdlib module
  hgext/remotefilelog/__init__.py:58: imports from hgext.remotefilelog not lexically sorted: remotefilectx < remotefilelog
  hgext/remotefilelog/__init__.py:59: multiple imported names: shallowbundle, debugcommands, remotefilelogserver, shallowverifier
  hgext/remotefilelog/__init__.py:60: multiple imported names: shallowutil, shallowrepo
  hgext/remotefilelog/__init__.py:61: imports not lexically sorted: repack < shallowutil
  hgext/remotefilelog/__init__.py:62: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/__init__.py:63: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/__init__.py:64: direct symbol import wrapfunction from mercurial.extensions
  hgext/remotefilelog/__init__.py:64: symbol import follows non-symbol import: mercurial.extensions
  hgext/remotefilelog/__init__.py:94: stdlib import "os" follows local import: mercurial
  hgext/remotefilelog/__init__.py:95: stdlib import "time" follows local import: mercurial
  hgext/remotefilelog/__init__.py:96: stdlib import "traceback" follows local import: mercurial
  hgext/remotefilelog/basepack.py:3: multiple imported names: errno, hashlib, mmap, os, struct, time
  hgext/remotefilelog/basepack.py:5: relative import of stdlib module
  hgext/remotefilelog/basepack.py:5: direct symbol import defaultdict from collections
  hgext/remotefilelog/basepack.py:7: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/basestore.py:3: multiple imported names: errno, hashlib, os, shutil, stat, time
  hgext/remotefilelog/basestore.py:15: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/basestore.py:16: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/cacheclient.py:15: multiple imported names: os, sys
  hgext/remotefilelog/constants.py:4: stdlib import "struct" follows local import: mercurial.i18n
  hgext/remotefilelog/contentstore.py:12: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/datapack.py:8: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/datapack.py:8: imports from mercurial.node not lexically sorted: hex < nullid
  hgext/remotefilelog/datapack.py:9: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/datapack.py:11: direct symbol import lz4compress, lz4decompress from hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/datapack.py:11: symbol import follows non-symbol import: hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/datapack.py:13: import should be relative: hgext.extlib
  hgext/remotefilelog/debugcommands.py:10: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/debugcommands.py:11: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/debugcommands.py:12: direct symbol import extutil from hgext3rd
  hgext/remotefilelog/debugcommands.py:12: symbol import follows non-symbol import: hgext3rd
  hgext/remotefilelog/debugcommands.py:21: direct symbol import repacklockvfs from hgext.remotefilelog.repack
  hgext/remotefilelog/debugcommands.py:21: symbol import follows non-symbol import: hgext.remotefilelog.repack
  hgext/remotefilelog/debugcommands.py:22: direct symbol import lz4decompress from hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/debugcommands.py:22: symbol import follows non-symbol import: hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/debugcommands.py:23: multiple imported names: hashlib, os
  hgext/remotefilelog/debugcommands.py:23: stdlib import "hashlib" follows local import: hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/fileserverclient.py:10: multiple imported names: hashlib, os, time, io, struct
  hgext/remotefilelog/fileserverclient.py:15: imports from mercurial.node not lexically sorted: bin < hex
  hgext/remotefilelog/fileserverclient.py:30: direct symbol import unioncontentstore from hgext.remotefilelog.contentstore
  hgext/remotefilelog/fileserverclient.py:30: symbol import follows non-symbol import: hgext.remotefilelog.contentstore
  hgext/remotefilelog/fileserverclient.py:31: direct symbol import unionmetadatastore from hgext.remotefilelog.metadatastore
  hgext/remotefilelog/fileserverclient.py:31: symbol import follows non-symbol import: hgext.remotefilelog.metadatastore
  hgext/remotefilelog/fileserverclient.py:32: direct symbol import lz4decompress from hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/fileserverclient.py:32: symbol import follows non-symbol import: hgext.remotefilelog.lz4wrapper
  hgext/remotefilelog/historypack.py:2: multiple imported names: hashlib, struct
  hgext/remotefilelog/historypack.py:4: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/lz4wrapper.py:3: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/lz4wrapper.py:5: stdlib import "lz4" follows local import: mercurial.i18n
  hgext/remotefilelog/metadatastore.py:2: multiple imported names: basestore, shallowutil
  hgext/remotefilelog/remotefilectx.py:13: imports from mercurial not lexically sorted: error < util
  hgext/remotefilelog/remotefilectx.py:13: imports from mercurial not lexically sorted: ancestor < error
  hgext/remotefilelog/remotefilectx.py:13: imports from mercurial not lexically sorted: extensions < phases
  hgext/remotefilelog/remotefilelog.py:15: multiple imported names: collections, os
  hgext/remotefilelog/remotefilelog.py:15: stdlib import "collections" follows local import: hgext.remotefilelog
  hgext/remotefilelog/remotefilelog.py:16: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/remotefilelog.py:17: imports from mercurial not lexically sorted: mdiff < revlog
  hgext/remotefilelog/remotefilelog.py:17: imports from mercurial not lexically sorted: ancestor < mdiff
  hgext/remotefilelog/remotefilelog.py:18: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/remotefilelogserver.py:9: imports from mercurial not lexically sorted: changegroup < wireproto
  hgext/remotefilelog/remotefilelogserver.py:9: imports from mercurial not lexically sorted: changelog < util
  hgext/remotefilelog/remotefilelogserver.py:10: imports from mercurial not lexically sorted: error < store
  hgext/remotefilelog/remotefilelogserver.py:11: direct symbol import wrapfunction from mercurial.extensions
  hgext/remotefilelog/remotefilelogserver.py:11: symbol import follows non-symbol import: mercurial.extensions
  hgext/remotefilelog/remotefilelogserver.py:13: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/remotefilelogserver.py:14: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/remotefilelogserver.py:22: multiple imported names: errno, stat, os, time
  hgext/remotefilelog/remotefilelogserver.py:22: stdlib import "errno" follows local import: hgext.remotefilelog
  hgext/remotefilelog/repack.py:4: relative import of stdlib module
  hgext/remotefilelog/repack.py:4: direct symbol import runshellcommand, flock from hgext3rd.extutil
  hgext/remotefilelog/repack.py:4: imports from hgext3rd.extutil not lexically sorted: flock < runshellcommand
  hgext/remotefilelog/repack.py:15: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/repack.py:19: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/repack.py:28: stdlib import "time" follows local import: hgext.remotefilelog
  hgext/remotefilelog/shallowbundle.py:10: stdlib import "os" follows local import: hgext.remotefilelog
  hgext/remotefilelog/shallowbundle.py:11: symbol import follows non-symbol import: mercurial.node
  hgext/remotefilelog/shallowbundle.py:12: imports from mercurial not lexically sorted: match < mdiff
  hgext/remotefilelog/shallowbundle.py:12: imports from mercurial not lexically sorted: bundlerepo < match
  hgext/remotefilelog/shallowbundle.py:13: imports from mercurial not lexically sorted: error < util
  hgext/remotefilelog/shallowbundle.py:14: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/shallowrepo.py:9: relative import of stdlib module
  hgext/remotefilelog/shallowrepo.py:9: direct symbol import runshellcommand from hgext3rd.extutil
  hgext/remotefilelog/shallowrepo.py:12: imports from mercurial not lexically sorted: match < util
  hgext/remotefilelog/shallowrepo.py:13: imports from hgext.remotefilelog not lexically sorted: remotefilectx < remotefilelog
  hgext/remotefilelog/shallowrepo.py:19: multiple imported names: constants, shallowutil
  hgext/remotefilelog/shallowrepo.py:20: direct symbol import remotefilelogcontentstore, unioncontentstore from contentstore
  hgext/remotefilelog/shallowrepo.py:20: symbol import follows non-symbol import: contentstore
  hgext/remotefilelog/shallowrepo.py:21: direct symbol import remotecontentstore from contentstore
  hgext/remotefilelog/shallowrepo.py:21: symbol import follows non-symbol import: contentstore
  hgext/remotefilelog/shallowrepo.py:22: direct symbol import remotefilelogmetadatastore, unionmetadatastore from metadatastore
  hgext/remotefilelog/shallowrepo.py:22: symbol import follows non-symbol import: metadatastore
  hgext/remotefilelog/shallowrepo.py:23: direct symbol import remotemetadatastore from metadatastore
  hgext/remotefilelog/shallowrepo.py:23: symbol import follows non-symbol import: metadatastore
  hgext/remotefilelog/shallowrepo.py:24: direct symbol import datapackstore from datapack
  hgext/remotefilelog/shallowrepo.py:24: symbol import follows non-symbol import: datapack
  hgext/remotefilelog/shallowrepo.py:25: direct symbol import historypackstore from historypack
  hgext/remotefilelog/shallowrepo.py:25: symbol import follows non-symbol import: historypack
  hgext/remotefilelog/shallowrepo.py:27: stdlib import "os" follows local import: historypack
  hgext/remotefilelog/shallowutil.py:9: multiple imported names: errno, hashlib, os, stat, struct, tempfile
  hgext/remotefilelog/shallowutil.py:11: relative import of stdlib module
  hgext/remotefilelog/shallowutil.py:11: direct symbol import defaultdict from collections
  hgext/remotefilelog/shallowutil.py:12: imports from mercurial not lexically sorted: error < util
  hgext/remotefilelog/shallowutil.py:13: symbol import follows non-symbol import: mercurial.i18n
  hgext/remotefilelog/shallowverifier.py:9: direct symbol import verifier from mercurial.verify
  hgext/remotefilelog/wirepack.py:12: stdlib import "struct" follows local import: constants
  hgext/remotefilelog/wirepack.py:13: relative import of stdlib module
  hgext/remotefilelog/wirepack.py:13: direct symbol import StringIO from StringIO
  hgext/remotefilelog/wirepack.py:14: relative import of stdlib module
  hgext/remotefilelog/wirepack.py:14: direct symbol import defaultdict from collections
  hgext/remotefilelog/wirepack.py:16: direct symbol import readexactly, readunpack, mkstickygroupdir, readpath from shallowutil
  hgext/remotefilelog/wirepack.py:16: imports from shallowutil not lexically sorted: mkstickygroupdir < readunpack
  hgext/remotefilelog/wirepack.py:17: multiple imported names: datapack, historypack, shallowutil
  hgext/smartlog.py:29: imports not lexically sorted: anydbm < contextlib
  hgext/smartlog.py:30: relative import of stdlib module
  hgext/smartlog.py:30: direct symbol import chain from itertools
  hgext/smartlog.py:53: symbol import follows non-symbol import: mercurial.i18n
  hgext/treedirstate.py:48: symbol import follows non-symbol import: mercurial.i18n
  hgext/treedirstate.py:49: stdlib import "errno" follows local import: mercurial.i18n
  hgext/treedirstate.py:50: stdlib import "heapq" follows local import: mercurial.i18n
  hgext/treedirstate.py:51: stdlib import "itertools" follows local import: mercurial.i18n
  hgext/treedirstate.py:52: stdlib import "os" follows local import: mercurial.i18n
  hgext/treedirstate.py:53: stdlib import "random" follows local import: mercurial.i18n
  hgext/treedirstate.py:54: stdlib import "struct" follows local import: mercurial.i18n
  hgext/treedirstate.py:55: imports not lexically sorted: string < struct
  hgext/treedirstate.py:55: stdlib import "string" follows local import: mercurial.i18n
  hgext/treedirstate.py:57: import should be relative: hgext.extlib.treedirstate
  hgext/treemanifest/__init__.py:99: symbol import follows non-symbol import: hgext.extlib
  hgext/treemanifest/__init__.py:100: symbol import follows non-symbol import: hgext.remotefilelog
  hgext/treemanifest/__init__.py:108: direct symbol import manifestrevlogstore, unioncontentstore from hgext.remotefilelog.contentstore
  hgext/treemanifest/__init__.py:108: symbol import follows non-symbol import: hgext.remotefilelog.contentstore
  hgext/treemanifest/__init__.py:112: direct symbol import unionmetadatastore from hgext.remotefilelog.metadatastore
  hgext/treemanifest/__init__.py:112: symbol import follows non-symbol import: hgext.remotefilelog.metadatastore
  hgext/treemanifest/__init__.py:115: direct symbol import datapack, datapackstore, mutabledatapack from hgext.remotefilelog.datapack
  hgext/treemanifest/__init__.py:115: symbol import follows non-symbol import: hgext.remotefilelog.datapack
  hgext/treemanifest/__init__.py:120: direct symbol import historypack, historypackstore, mutablehistorypack from hgext.remotefilelog.historypack
  hgext/treemanifest/__init__.py:120: symbol import follows non-symbol import: hgext.remotefilelog.historypack
  hgext/treemanifest/__init__.py:125: direct symbol import _computeincrementaldatapack, _computeincrementalhistorypack, _runrepack, _topacks, backgroundrepack from hgext.remotefilelog.repack
  hgext/treemanifest/__init__.py:125: symbol import follows non-symbol import: hgext.remotefilelog.repack
  hgext/tweakdefaults.py:69: imports from mercurial not lexically sorted: encoding < error
  hgext/tweakdefaults.py:87: import should be relative: hgext
  hgext/tweakdefaults.py:88: stdlib import "inspect" follows local import: hgext
  hgext/tweakdefaults.py:89: stdlib import "json" follows local import: hgext
  hgext/tweakdefaults.py:90: stdlib import "os" follows local import: hgext
  hgext/tweakdefaults.py:91: stdlib import "re" follows local import: hgext
  hgext/tweakdefaults.py:92: stdlib import "shlex" follows local import: hgext
  hgext/tweakdefaults.py:93: stdlib import "subprocess" follows local import: hgext
  hgext/tweakdefaults.py:94: imports not lexically sorted: stat < subprocess
  hgext/tweakdefaults.py:94: stdlib import "stat" follows local import: hgext
  hgext/tweakdefaults.py:95: stdlib import "time" follows local import: hgext
  hgext/tweakdefaults.py:1058: multiple imported names: msvcrt, _subprocess
  hgext/undo.py:39: symbol import follows non-symbol import: mercurial.node
  hgext/undo.py:45: direct symbol import interactiveui from hgext3rd
  hgext/undo.py:45: symbol import follows non-symbol import: hgext3rd
  tests/getflogheads.py:3: symbol import follows non-symbol import: mercurial.i18n
  tests/test-fb-hgext-fastannotate-revmap.py:10: relative import of stdlib module
  tests/test-fb-hgext-fastannotate-revmap.py:10: direct symbol import error, revmap from hgext3rd.fastannotate
  tests/test-fb-hgext-remotefilelog-datapack.py:19: imports not lexically sorted: pythonpath < silenttestrunner
  tests/test-fb-hgext-remotefilelog-datapack.py:22: relative import of stdlib module
  tests/test-fb-hgext-remotefilelog-datapack.py:22: direct symbol import datapack, datapackstore, fastdatapack, mutabledatapack from remotefilelog.datapack
  tests/test-fb-hgext-remotefilelog-datapack.py:28: relative import of stdlib module
  tests/test-fb-hgext-remotefilelog-datapack.py:28: direct symbol import SMALLFANOUTCUTOFF, SMALLFANOUTPREFIX, LARGEFANOUTPREFIX from remotefilelog.basepack
  tests/test-fb-hgext-remotefilelog-datapack.py:28: imports from remotefilelog.basepack not lexically sorted: LARGEFANOUTPREFIX < SMALLFANOUTPREFIX
  tests/test-fb-hgext-remotefilelog-datapack.py:33: relative import of stdlib module
  tests/test-fb-hgext-remotefilelog-datapack.py:33: direct symbol import constants from remotefilelog
  tests/test-fb-hgext-remotefilelog-datapack.py:36: imports not lexically sorted: mercurial.ui < pythonpath
  tests/test-fb-hgext-remotefilelog-histpack.py:18: imports not lexically sorted: pythonpath < silenttestrunner
  tests/test-fb-hgext-remotefilelog-histpack.py:21: relative import of stdlib module
  tests/test-fb-hgext-remotefilelog-histpack.py:21: direct symbol import historypack, mutablehistorypack from remotefilelog.historypack
  tests/test-fb-hgext-remotefilelog-histpack.py:24: imports not lexically sorted: mercurial.ui < pythonpath
  tests/test-fb-hgext-remotefilelog-histpack.py:26: relative import of stdlib module
  tests/test-fb-hgext-remotefilelog-histpack.py:26: direct symbol import SMALLFANOUTCUTOFF, LARGEFANOUTPREFIX from remotefilelog.basepack
  tests/test-fb-hgext-remotefilelog-histpack.py:26: imports from remotefilelog.basepack not lexically sorted: LARGEFANOUTPREFIX < SMALLFANOUTCUTOFF
  tests/test-fb-hgext-undo.t:215: imports from mercurial not lexically sorted: merge < registrar
  tests/test-fb-hgext-undo.t:215: imports from mercurial not lexically sorted: encoding < merge
  Import cycle: fb-hgext.fastmanifest.cachemanager -> fb-hgext.fastmanifest.implementation -> fb-hgext.fastmanifest.cachemanager
  [1]
