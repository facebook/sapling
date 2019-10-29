#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg files . | egrep -v "^(edenscm/hgext/extlib/pywatchman|lib/cdatapack|lib/third-party|edenscm/mercurial/thirdparty|fb|newdoc)" \
  > | sed 's-\\-/-g' > $TESTTMP/files.txt

  $ NPROC=`python -c 'import multiprocessing; print(multiprocessing.cpu_count())'`
  $ cat $TESTTMP/files.txt | xargs -n64 -P $NPROC contrib/check-code.py --warnings --per-file=0 | sort
  Skipping edenscm/hgext/extlib/cstore/datapackstore.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/datapackstore.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/datastore.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/deltachain.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/deltachain.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/key.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/match.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/py-cstore.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/py-datapackstore.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/py-structs.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/py-treemanifest.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/pythondatastore.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/pythondatastore.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/pythonkeyiterator.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/pythonutil.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/pythonutil.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/store.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/uniondatapackstore.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/cstore/uniondatapackstore.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_entry.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_entry.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_fetcher.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_ptr.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/manifest_ptr.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/treemanifest.cpp it has no-che?k-code (glob)
  Skipping edenscm/hgext/extlib/ctreemanifest/treemanifest.h it has no-che?k-code (glob)
  Skipping edenscm/hgext/globalrevs.py it has no-che?k-code (glob)
  Skipping edenscm/hgext/hgsql.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/statprof.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping lib/clib/buffer.c it has no-che?k-code (glob)
  Skipping lib/clib/buffer.h it has no-che?k-code (glob)
  Skipping lib/clib/convert.h it has no-che?k-code (glob)
  Skipping lib/clib/null_test.c it has no-che?k-code (glob)
  Skipping lib/clib/portability/dirent.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/inet.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/mman.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/portability.h it has no-che?k-code (glob)
  Skipping lib/clib/portability/unistd.h it has no-che?k-code (glob)
  Skipping lib/clib/sha1.h it has no-che?k-code (glob)
  Skipping tests/badserverext.py it has no-che?k-code (glob)
  Skipping tests/conduithttp.py it has no-che?k-code (glob)
  Skipping tests/test-fb-hgext-remotefilelog-bad-configs.t it has no-che?k-code (glob)
  Skipping tests/test-hgsql-encoding.t it has no-che?k-code (glob)
  Skipping tests/test-hgsql-race-conditions.t it has no-che?k-code (glob)
  Skipping tests/test-rustthreading.py it has no-che?k-code (glob)
  edenscm/mercurial/commands/eden.py:408: use foobar, not foo_bar naming --> def cmd_get_file_size(self, request):
  tests/run-tests.py:*: don't use camelcase in identifiers --> self.testsSkipped = 0 (glob)

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('edenscm/mercurial/commands/debug.py', 'rb') as fh:
  ...     for line in fh:
  ...         m = re.match("^@command\('([a-z]+)", line)
  ...         if m:
  ...             commands.append(m.group(1))
  >>> scommands = list(sorted(commands))
  >>> for i, command in enumerate(scommands):
  ...     if command != commands[i]:
  ...         print('commands in debugcommands.py not sorted; first differing '
  ...               'command is %s; expected %s' % (commands[i], command))
  ...         break

