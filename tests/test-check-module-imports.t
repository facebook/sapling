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
  fb-hgext/hgext3rd/absorb/__init__.py:50: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/age.py:21: imports not lexically sorted: re < time
  fb-hgext/hgext3rd/cleanobsstore.py:37: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/conflictinfo.py:30: direct symbol import absentfilectx from mercurial.filemerge
  fb-hgext/hgext3rd/crdump.py:5: multiple imported names: json, re, shutil, tempfile
  fb-hgext/hgext3rd/crdump.py:6: relative import of stdlib module
  fb-hgext/hgext3rd/crdump.py:6: direct symbol import path from os
  fb-hgext/hgext3rd/crdump.py:16: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/crdump.py:17: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/dirsync.py:46: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fastannotate/context.py:23: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fastannotate/context.py:30: imports not lexically sorted: linelog < os
  fb-hgext/hgext3rd/fastannotate/context.py:30: stdlib import "linelog" follows local import: fb-hgext.hgext3rd.fastannotate
  fb-hgext/hgext3rd/fastannotate/revmap.py:16: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/fastverify.py:24: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/__init__.py:58: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/fbamend/__init__.py:60: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/__init__.py:81: stdlib import "tempfile" follows local import: fb-hgext.hgext3rd.fbamend
  fb-hgext/hgext3rd/fbamend/common.py:10: relative import of stdlib module
  fb-hgext/hgext3rd/fbamend/common.py:10: direct symbol import defaultdict from collections
  fb-hgext/hgext3rd/fbamend/common.py:21: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/common.py:22: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/fbamend/fold.py:23: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/hiddenoverride.py:10: direct symbol import extutil from hgext3rd
  fb-hgext/hgext3rd/fbamend/hide.py:12: imports from mercurial not lexically sorted: extensions < hg
  fb-hgext/hgext3rd/fbamend/hide.py:22: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/metaedit.py:23: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/movement.py:10: relative import of stdlib module
  fb-hgext/hgext3rd/fbamend/movement.py:10: direct symbol import count from itertools
  fb-hgext/hgext3rd/fbamend/movement.py:21: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/movement.py:22: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/fbamend/prune.py:14: imports from mercurial not lexically sorted: repair < util
  fb-hgext/hgext3rd/fbamend/prune.py:26: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/split.py:25: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/fbamend/unamend.py:17: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/gitrevset.py:25: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/gitrevset.py:26: stdlib import "re" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/hiddenerror.py:28: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/hiddenerror.py:29: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/lfs/__init__.py:43: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/lfs/blobstore.py:21: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/lfs/pointer.py:15: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/lfs/wrapper.py:18: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/lfs/wrapper.py:19: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/p4fastimport/__init__.py:35: imports from fb-hgext.hgext3rd.p4fastimport not lexically sorted: importer < p4
  fb-hgext/hgext3rd/p4fastimport/__init__.py:35: imports from fb-hgext.hgext3rd.p4fastimport not lexically sorted: filetransaction < importer
  fb-hgext/hgext3rd/p4fastimport/__init__.py:41: direct symbol import runworker, lastcl, decodefileflags from fb-hgext.hgext3rd.p4fastimport.util
  fb-hgext/hgext3rd/p4fastimport/__init__.py:41: symbol import follows non-symbol import: fb-hgext.hgext3rd.p4fastimport.util
  fb-hgext/hgext3rd/p4fastimport/__init__.py:41: imports from fb-hgext.hgext3rd.p4fastimport.util not lexically sorted: lastcl < runworker
  fb-hgext/hgext3rd/p4fastimport/__init__.py:41: imports from fb-hgext.hgext3rd.p4fastimport.util not lexically sorted: decodefileflags < lastcl
  fb-hgext/hgext3rd/p4fastimport/__init__.py:43: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/p4fastimport/__init__.py:44: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/p4fastimport/__init__.py:44: imports from mercurial.node not lexically sorted: hex < short
  fb-hgext/hgext3rd/p4fastimport/importer.py:19: direct symbol import caseconflict, localpath from fb-hgext.hgext3rd.p4fastimport.util
  fb-hgext/hgext3rd/p4fastimport/importer.py:19: symbol import follows non-symbol import: fb-hgext.hgext3rd.p4fastimport.util
  fb-hgext/hgext3rd/p4fastimport/p4.py:11: direct symbol import runworker from fb-hgext.hgext3rd.p4fastimport.util
  fb-hgext/hgext3rd/pushrebase.py:27: multiple imported names: errno, os, tempfile, mmap, time
  fb-hgext/hgext3rd/pushrebase.py:49: direct symbol import wrapcommand, wrapfunction, unwrapfunction from mercurial.extensions
  fb-hgext/hgext3rd/pushrebase.py:49: symbol import follows non-symbol import: mercurial.extensions
  fb-hgext/hgext3rd/pushrebase.py:49: imports from mercurial.extensions not lexically sorted: unwrapfunction < wrapfunction
  fb-hgext/hgext3rd/pushrebase.py:50: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/pushrebase.py:50: imports from mercurial.node not lexically sorted: hex < nullid
  fb-hgext/hgext3rd/pushrebase.py:50: imports from mercurial.node not lexically sorted: bin < hex
  fb-hgext/hgext3rd/pushrebase.py:51: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/pushrebase.py:53: relative import of stdlib module
  fb-hgext/hgext3rd/pushrebase.py:53: direct symbol import contentstore, datapack, historypack, metadatastore, wirepack from remotefilelog
  fb-hgext/hgext3rd/pushrebase.py:53: symbol import follows non-symbol import: remotefilelog
  fb-hgext/hgext3rd/smartlog.py:29: imports not lexically sorted: anydbm < contextlib
  fb-hgext/hgext3rd/smartlog.py:30: relative import of stdlib module
  fb-hgext/hgext3rd/smartlog.py:30: direct symbol import chain from itertools
  fb-hgext/hgext3rd/smartlog.py:53: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:48: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:49: stdlib import "errno" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:50: stdlib import "heapq" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:51: stdlib import "itertools" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:52: stdlib import "os" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:53: stdlib import "random" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:54: stdlib import "struct" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:55: imports not lexically sorted: string < struct
  fb-hgext/hgext3rd/treedirstate.py:55: stdlib import "string" follows local import: mercurial.i18n
  fb-hgext/hgext3rd/treedirstate.py:57: relative import of stdlib module
  fb-hgext/hgext3rd/treedirstate.py:57: direct symbol import treedirstatemap from hgext3rd.rust.treedirstate
  fb-hgext/hgext3rd/treedirstate.py:57: symbol import follows non-symbol import: hgext3rd.rust.treedirstate
  fb-hgext/hgext3rd/tweakdefaults.py:70: imports from mercurial not lexically sorted: encoding < error
  fb-hgext/hgext3rd/tweakdefaults.py:89: stdlib import "inspect" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:90: stdlib import "json" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:91: stdlib import "os" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:92: stdlib import "re" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:93: stdlib import "shlex" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:94: stdlib import "subprocess" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:95: imports not lexically sorted: stat < subprocess
  fb-hgext/hgext3rd/tweakdefaults.py:95: stdlib import "stat" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:96: stdlib import "time" follows local import: hgext
  fb-hgext/hgext3rd/tweakdefaults.py:1059: multiple imported names: msvcrt, _subprocess
  fb-hgext/hgext3rd/undo.py:39: symbol import follows non-symbol import: mercurial.node
  fb-hgext/hgext3rd/undo.py:45: direct symbol import interactiveui from hgext3rd
  fb-hgext/hgext3rd/undo.py:45: symbol import follows non-symbol import: hgext3rd
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
  fb-hgext/remotefilelog/basepack.py:3: multiple imported names: errno, hashlib, mmap, os, struct, time
  fb-hgext/remotefilelog/basepack.py:5: relative import of stdlib module
  fb-hgext/remotefilelog/basepack.py:5: direct symbol import defaultdict from collections
  fb-hgext/remotefilelog/basepack.py:7: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/basestore.py:3: multiple imported names: errno, hashlib, os, shutil, stat, time
  fb-hgext/remotefilelog/basestore.py:14: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/basestore.py:15: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/contentstore.py:12: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/datapack.py:8: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/datapack.py:8: imports from mercurial.node not lexically sorted: hex < nullid
  fb-hgext/remotefilelog/datapack.py:9: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/datapack.py:11: direct symbol import lz4compress, lz4decompress from fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/datapack.py:11: symbol import follows non-symbol import: fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/debugcommands.py:10: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/debugcommands.py:11: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/debugcommands.py:12: direct symbol import extutil from hgext3rd
  fb-hgext/remotefilelog/debugcommands.py:12: symbol import follows non-symbol import: hgext3rd
  fb-hgext/remotefilelog/debugcommands.py:21: direct symbol import repacklockvfs from fb-hgext.remotefilelog.repack
  fb-hgext/remotefilelog/debugcommands.py:21: symbol import follows non-symbol import: fb-hgext.remotefilelog.repack
  fb-hgext/remotefilelog/debugcommands.py:22: direct symbol import lz4decompress from fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/debugcommands.py:22: symbol import follows non-symbol import: fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/debugcommands.py:23: multiple imported names: hashlib, os
  fb-hgext/remotefilelog/debugcommands.py:23: stdlib import "hashlib" follows local import: fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/fileserverclient.py:10: multiple imported names: hashlib, os, time, io, struct
  fb-hgext/remotefilelog/fileserverclient.py:15: imports from mercurial.node not lexically sorted: bin < hex
  fb-hgext/remotefilelog/fileserverclient.py:30: direct symbol import unioncontentstore from fb-hgext.remotefilelog.contentstore
  fb-hgext/remotefilelog/fileserverclient.py:30: symbol import follows non-symbol import: fb-hgext.remotefilelog.contentstore
  fb-hgext/remotefilelog/fileserverclient.py:31: direct symbol import unionmetadatastore from fb-hgext.remotefilelog.metadatastore
  fb-hgext/remotefilelog/fileserverclient.py:31: symbol import follows non-symbol import: fb-hgext.remotefilelog.metadatastore
  fb-hgext/remotefilelog/fileserverclient.py:32: direct symbol import lz4decompress from fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/fileserverclient.py:32: symbol import follows non-symbol import: fb-hgext.remotefilelog.lz4wrapper
  fb-hgext/remotefilelog/remotefilelog.py:15: multiple imported names: collections, os
  fb-hgext/remotefilelog/remotefilelog.py:15: stdlib import "collections" follows local import: fb-hgext.remotefilelog
  fb-hgext/remotefilelog/remotefilelog.py:16: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/remotefilelog.py:17: imports from mercurial not lexically sorted: mdiff < revlog
  fb-hgext/remotefilelog/remotefilelog.py:17: imports from mercurial not lexically sorted: ancestor < mdiff
  fb-hgext/remotefilelog/remotefilelog.py:18: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/remotefilelogserver.py:9: imports from mercurial not lexically sorted: changegroup < wireproto
  fb-hgext/remotefilelog/remotefilelogserver.py:9: imports from mercurial not lexically sorted: changelog < util
  fb-hgext/remotefilelog/remotefilelogserver.py:10: imports from mercurial not lexically sorted: error < store
  fb-hgext/remotefilelog/remotefilelogserver.py:11: direct symbol import wrapfunction from mercurial.extensions
  fb-hgext/remotefilelog/remotefilelogserver.py:11: symbol import follows non-symbol import: mercurial.extensions
  fb-hgext/remotefilelog/remotefilelogserver.py:13: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/remotefilelogserver.py:14: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/remotefilelogserver.py:22: multiple imported names: errno, stat, os, time
  fb-hgext/remotefilelog/remotefilelogserver.py:22: stdlib import "errno" follows local import: fb-hgext.remotefilelog
  fb-hgext/remotefilelog/repack.py:4: relative import of stdlib module
  fb-hgext/remotefilelog/repack.py:4: direct symbol import runshellcommand, flock from hgext3rd.extutil
  fb-hgext/remotefilelog/repack.py:4: imports from hgext3rd.extutil not lexically sorted: flock < runshellcommand
  fb-hgext/remotefilelog/repack.py:14: symbol import follows non-symbol import: mercurial.node
  fb-hgext/remotefilelog/repack.py:18: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/remotefilelog/repack.py:27: stdlib import "time" follows local import: fb-hgext.remotefilelog
  fb-hgext/remotefilelog/shallowutil.py:9: multiple imported names: errno, hashlib, os, stat, struct, tempfile
  fb-hgext/remotefilelog/shallowutil.py:11: relative import of stdlib module
  fb-hgext/remotefilelog/shallowutil.py:11: direct symbol import defaultdict from collections
  fb-hgext/remotefilelog/shallowutil.py:12: imports from mercurial not lexically sorted: error < util
  fb-hgext/remotefilelog/shallowutil.py:13: symbol import follows non-symbol import: mercurial.i18n
  fb-hgext/scripts/traceprof.py:5: direct symbol import traceprof from hgext3rd
  fb-hgext/scripts/traceprof.py:6: ui from mercurial must be "as" aliased to uimod
  fb-hgext/scripts/traceprof.py:8: stdlib import "os" follows local import: mercurial
  fb-hgext/scripts/traceprof.py:9: stdlib import "sys" follows local import: mercurial
  fb-hgext/tests/check-ext.py:4: relative import of stdlib module
  fb-hgext/tests/check-ext.py:4: direct symbol import glob from glob
  fb-hgext/tests/test-cstore-datapackstore.py:17: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-cstore-datapackstore.py:20: relative import of stdlib module
  fb-hgext/tests/test-cstore-datapackstore.py:20: direct symbol import datapackstore from cstore
  fb-hgext/tests/test-cstore-datapackstore.py:24: relative import of stdlib module
  fb-hgext/tests/test-cstore-datapackstore.py:24: direct symbol import fastdatapack, mutabledatapack from remotefilelog.datapack
  fb-hgext/tests/test-cstore-datapackstore.py:30: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-cstore-treemanifest.py:13: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-cstore-treemanifest.py:16: stdlib import "cstore" follows local import: pythonpath
  fb-hgext/tests/test-cstore-treemanifest.py:22: symbol import follows non-symbol import: mercurial.node
  fb-hgext/tests/test-cstore-uniondatapackstore.py:16: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-cstore-uniondatapackstore.py:19: relative import of stdlib module
  fb-hgext/tests/test-cstore-uniondatapackstore.py:19: direct symbol import datapackstore, uniondatapackstore from cstore
  fb-hgext/tests/test-cstore-uniondatapackstore.py:24: relative import of stdlib module
  fb-hgext/tests/test-cstore-uniondatapackstore.py:24: direct symbol import datapack, mutabledatapack from remotefilelog.datapack
  fb-hgext/tests/test-cstore-uniondatapackstore.py:30: symbol import follows non-symbol import: mercurial.node
  fb-hgext/tests/test-cstore-uniondatapackstore.py:31: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-fastannotate-revmap.py:10: relative import of stdlib module
  fb-hgext/tests/test-fastannotate-revmap.py:10: direct symbol import error, revmap from hgext3rd.fastannotate
  fb-hgext/tests/test-lfs-pointer.py:9: relative import of stdlib module
  fb-hgext/tests/test-lfs-pointer.py:9: direct symbol import pointer from hgext3rd.lfs
  fb-hgext/tests/test-patchrmdir.py:13: direct symbol import patchrmdir from hgext3rd
  fb-hgext/tests/test-remotefilelog-datapack.py:19: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-remotefilelog-datapack.py:22: relative import of stdlib module
  fb-hgext/tests/test-remotefilelog-datapack.py:22: direct symbol import datapack, datapackstore, fastdatapack, mutabledatapack from remotefilelog.datapack
  fb-hgext/tests/test-remotefilelog-datapack.py:28: relative import of stdlib module
  fb-hgext/tests/test-remotefilelog-datapack.py:28: direct symbol import SMALLFANOUTCUTOFF, SMALLFANOUTPREFIX, LARGEFANOUTPREFIX from remotefilelog.basepack
  fb-hgext/tests/test-remotefilelog-datapack.py:28: imports from remotefilelog.basepack not lexically sorted: LARGEFANOUTPREFIX < SMALLFANOUTPREFIX
  fb-hgext/tests/test-remotefilelog-datapack.py:33: relative import of stdlib module
  fb-hgext/tests/test-remotefilelog-datapack.py:33: direct symbol import constants from remotefilelog
  fb-hgext/tests/test-remotefilelog-datapack.py:36: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-remotefilelog-histpack.py:18: imports not lexically sorted: pythonpath < silenttestrunner
  fb-hgext/tests/test-remotefilelog-histpack.py:21: relative import of stdlib module
  fb-hgext/tests/test-remotefilelog-histpack.py:21: direct symbol import historypack, mutablehistorypack from remotefilelog.historypack
  fb-hgext/tests/test-remotefilelog-histpack.py:24: imports not lexically sorted: mercurial.ui < pythonpath
  fb-hgext/tests/test-remotefilelog-histpack.py:26: relative import of stdlib module
  fb-hgext/tests/test-remotefilelog-histpack.py:26: direct symbol import SMALLFANOUTCUTOFF, LARGEFANOUTPREFIX from remotefilelog.basepack
  fb-hgext/tests/test-remotefilelog-histpack.py:26: imports from remotefilelog.basepack not lexically sorted: LARGEFANOUTPREFIX < SMALLFANOUTCUTOFF
  fb-hgext/tests/test-undo.t:215: imports from mercurial not lexically sorted: merge < registrar
  fb-hgext/tests/test-undo.t:215: imports from mercurial not lexically sorted: encoding < merge
  Import cycle: fb-hgext.fastmanifest.cachemanager -> fb-hgext.fastmanifest.implementation -> fb-hgext.fastmanifest.cachemanager
  [1]
