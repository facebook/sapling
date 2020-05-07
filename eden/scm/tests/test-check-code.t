#chg-compatible

#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg files . | egrep -v "^(edenscm/hgext/extlib/pywatchman|lib/cdatapack|lib/third-party|edenscm/mercurial/thirdparty|fb|newdoc|tests/gpg|tests/bundles|edenscm/mercurial/templates/static|i18n|slides|tests/hggit/latin-1-encoding|.*\\.(bin|bindag|hg|pdf|jpg)$)" \
  > | sed 's-\\-/-g' > $TESTTMP/files.txt

  $ NPROC=`$PYTHON -c 'import multiprocessing; print(multiprocessing.cpu_count())'`
  $ cat $TESTTMP/files.txt | PYTHONPATH= xargs -n64 -P $NPROC contrib/check-code.py --warnings --per-file=0 | sort
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
  Skipping edenscm/mercurial/commands/eden.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping edenscm/mercurial/statprof.py it has no-che?k-code (glob)
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
  edenscm/hgext/extlib/phabricator/graphql.py:*: use foobar, not foo_bar naming --> ca_bundle = repo.ui.configpath("web", "cacerts") (glob)
  edenscm/hgext/extlib/phabricator/graphql.py:*: use foobar, not foo_bar naming --> def scmquery_log( (glob)
  edenscm/hgext/hggit/git_handler.py:*: use foobar, not foo_bar naming --> git_renames = {} (glob)
  tests/run-tests.py:*: don't use camelcase in identifiers --> self.testsSkipped = 0 (glob)
  tests/test-absorb-t.py:325: always assign an opened file to a variable, and close it afterwards --> open(f, "ab").write(line.encode("utf-8"))
  tests/test-absorb-t.py:40: always assign an opened file to a variable, and close it afterwards --> content = open(path, "rb").read().replace(a, b)
  tests/test-absorb-t.py:41: always assign an opened file to a variable, and close it afterwards --> open(path, "wb").write(content)
  tests/test-absorb-t.py:458: always assign an opened file to a variable, and close it afterwards --> open("c", "wb").write(bytearray([0, 1, 2, 10]))
  tests/test-absorb-t.py:57: always assign an opened file to a variable, and close it afterwards --> open("a", "ab").write(b"%s\n" % i)
  tests/test-adding-invalid-utf8-t.py:20: always assign an opened file to a variable, and close it afterwards --> open("\x9d\xc8\xac\xde\xa1\xee", "wb").write("test")
  tests/test-amend-restack-t.py:17: always assign an opened file to a variable, and close it afterwards --> open(name, "wb").write(b"%s\n" % name.encode("utf8"))
  tests/test-check-win32-signature.py:13: always assign an opened file to a variable, and close it afterwards --> content = open(path).read()
  tests/test-command-template-t.py:2011: always assign an opened file to a variable, and close it afterwards --> open("a", "wb").write(s)
  tests/test-command-template-t.py:3257: always assign an opened file to a variable, and close it afterwards --> open("a", "wb").write("%s\n" % i)
  tests/test-command-template-t.py:3914: always assign an opened file to a variable, and close it afterwards --> open("utf-8", "wb").write(utf8)
  tests/test-diff-antipatience-t.py:14: always assign an opened file to a variable, and close it afterwards --> open("a", "w").write("\n".join(list("a" + "x" * 10 + "u" + "x" * 30 + "a\n")))
  tests/test-diff-antipatience-t.py:16: always assign an opened file to a variable, and close it afterwards --> open("a", "w").write("\n".join(list("b" + "x" * 30 + "u" + "x" * 10 + "b\n")))
  tests/test-export-t.py:20: always assign an opened file to a variable, and close it afterwards --> open("foo", "ab").write("foo-%s\n" % i)
  tests/test-extension-inline.t:12: don't use 'python', use '$PYTHON' --> $ setconfig "extensions.foo=python-base64:`python -c 'import base64; print(base64.b64encode(open(\"foo.py\", "rb").read()).decode("utf-8").replace(\"\\n\",\"\"))'`"
  tests/test-fb-hgext-fastannotate-revmap.py:151: always assign an opened file to a variable, and close it afterwards --> ensure(len(set(open(p).read() for p in [path, path2])) == 1)
  tests/test-fb-hgext-merge-conflictinfo.t:81: don't use 'python', use '$PYTHON' --> >  local result=`hg resolve --tool internal:dumpjson --all | python -c "$script"`
  tests/test-fb-hgext-patchrmdir.py:47: always assign an opened file to a variable, and close it afterwards --> open(d2, "w").close()
  tests/test-gitlookup-infinitepush.t:10: don't use 'python', use '$PYTHON' --> $ echo 'ssh = python "$RUNTESTDIR/dummyssh"' >> $HGRCPATH
  tests/test-glog-t.py:89: always assign an opened file to a variable, and close it afterwards --> open("a", "wb").write("%s\n" % rev)
  tests/test-hgsql-pushrebase2.t:13: don't use 'python', use '$PYTHON' --> $ setconfig hgsql.initialsync=false treemanifest.treeonly=1 treemanifest.sendtrees=1 remotefilelog.reponame=x remotefilelog.cachepath=$TESTTMP/cache ui.ssh="python $TESTDIR/dummyssh" pushrebase.verbose=1 experimental.bundle2lazylocking=True
  tests/test-hgsql-requires.t:34: don't use 'python', use '$PYTHON' --> $ hg clone --config extensions.hgsql=! --config ui.ssh='python "$TESTDIR/dummyssh"' --uncompressed ssh://user@dummy/master client2 | grep "streaming all changes"
  tests/test-hgsql-requires.t:38: don't use 'python', use '$PYTHON' --> $ hg clone --config extensions.hgsql= --config ui.ssh='python "$TESTDIR/dummyssh"' --uncompressed ssh://user@dummy/master client3
  tests/test-import-t.py:168: always assign an opened file to a variable, and close it afterwards --> content = open("diffed-tip.patch", "rb").read().replace(b"1,1", b"foo")
  tests/test-import-t.py:169: always assign an opened file to a variable, and close it afterwards --> open("broken.patch", "wb").write(content)
  tests/test-import-t.py:219: always assign an opened file to a variable, and close it afterwards --> patch = open(path1, "rb").read()
  tests/test-import-t.py:223: always assign an opened file to a variable, and close it afterwards --> open(path2, "wb").write(msg.as_string().encode("utf-8"))
  tests/test-import-t.py:257: always assign an opened file to a variable, and close it afterwards --> patch = open(path1, "rb").read()
  tests/test-import-t.py:261: always assign an opened file to a variable, and close it afterwards --> open(path2, "wb").write(msg.as_string().encode("utf-8"))
  tests/test-import-t.py:341: always assign an opened file to a variable, and close it afterwards --> open("subdir-tip.patch", "wb").write(open("tmp", "rb").read().replace(b"d1/d2", b""))
  tests/test-import-t.py:473: always assign an opened file to a variable, and close it afterwards --> open("b", "wb").write(b"a\0b")
  tests/test-import-t.py:747: always assign an opened file to a variable, and close it afterwards --> open("trickyheaders.patch", "wb").write(
  tests/test-infinitepush-bundlestore.t:227: don't use 'python', use '$PYTHON' --> > ssh = python "$TESTDIR/dummyssh"
  tests/test-infinitepush-bundlestore.t:258: don't use 'python', use '$PYTHON' --> > ssh = python "$TESTDIR/dummyssh"
  tests/test-memcommit.t:186: don't use 'python', use '$PYTHON' --> >   ( cd "$1" && setconfig ui.ssh="python \"$TESTDIR/dummyssh\"" )
  tests/test-obsmarker-template-t.py:44: always assign an opened file to a variable, and close it afterwards --> open(name, "wb").write(pycompat.encodeutf8("%s\n" % name))
  tests/test-remotenames-selective-pull-accessed-bookmarks.t:40: don't use 'python', use '$PYTHON' --> >        sort -k 3 $file ; python $TESTTMP/verifylast.py
  tests/test-revert-t.py:24: always assign an opened file to a variable, and close it afterwards --> content = open(filename).read()
  tests/test-revset-age-t.py:26: always assign an opened file to a variable, and close it afterwards --> open("file1", "w").write("%s\n" % delta)
  tests/test-revset-t.py:1798: always assign an opened file to a variable, and close it afterwards --> open("a", "wb").write("%s\n" % i)
  tests/test-shelve-t.py:1025: always assign an opened file to a variable, and close it afterwards --> f.write(open(".hg/shelvedstate").read().replace("ae8c668541e8", "123456789012"))
  tests/test-sparse-fetch-t.py:136: always assign an opened file to a variable, and close it afterwards --> open("y", "w").write("2")
  tests/test-sparse-fetch-t.py:138: always assign an opened file to a variable, and close it afterwards --> open("z/1", "w").write("2")
  tests/test-sparse-fetch-t.py:139: always assign an opened file to a variable, and close it afterwards --> open("z/z", "w").write("2")
  tests/testutil/argspans.py:30: always assign an opened file to a variable, and close it afterwards --> return parso.parse(open(path).read())
  tests/testutil/autofix.py:125: always assign an opened file to a variable, and close it afterwards --> lines = open(path, "rb").read().decode("utf-8").splitlines(True)
  tests/testutil/dott/shlib/__init__.py:219: always assign an opened file to a variable, and close it afterwards --> open(path, "a")
  tests/testutil/dott/shlib/__init__.py:234: always assign an opened file to a variable, and close it afterwards --> linecount += len(open(arg).read().splitlines())
  tests/testutil/dott/shlib/__init__.py:250: always assign an opened file to a variable, and close it afterwards --> stdin += open(path).read()
  tests/testutil/dott/shlib/__init__.py:80: always assign an opened file to a variable, and close it afterwards --> content = "".join(open(path).read() for path in args)
  tests/testutil/dott/shlib/hgsql.py:106: always assign an opened file to a variable, and close it afterwards --> open(os.path.join(name, ".hg/hgrc"), "ab").write(
  tests/testutil/dott/shlib/hgsql.py:73: always assign an opened file to a variable, and close it afterwards --> open(os.path.join(servername, ".hg/hgrc"), "ab").write(
  tests/testutil/dott/shlib/remotefilelog.py:15: always assign an opened file to a variable, and close it afterwards --> open(testtmp.HGRCPATH, "a").write(
  tests/testutil/dott/shlib/remotefilelog.py:51: always assign an opened file to a variable, and close it afterwards --> open(os.path.join(dest, ".hg/hgrc"), "ab").write(
  tests/testutil/dott/shlib/remotefilelog.py:72: always assign an opened file to a variable, and close it afterwards --> open(os.path.join(dest, ".hg/hgrc"), "ab").write(
  tests/testutil/dott/shlib/remotefilelog.py:98: always assign an opened file to a variable, and close it afterwards --> open(name, "wb").write("%s\n" % name)
  tests/testutil/dott/shobj.py:71: always assign an opened file to a variable, and close it afterwards --> open(outpath, mode).write(self._output.encode("utf-8"))
  tests/testutil/dott/testtmp.py:68: always assign an opened file to a variable, and close it afterwards --> open(hgrcpath, "w").write(
  tests/testutil/dott/translate.py:218: always assign an opened file to a variable, and close it afterwards --> code = open(path).read()
  tests/testutil/dott/translate.py:278: always assign an opened file to a variable, and close it afterwards --> open(cachepath, "w").write(repr(result))

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('edenscm/mercurial/commands/debug.py', 'rb') as fh:
  ...     for line in fh:
  ...         m = re.match(b"^@command\('([a-z]+)", line)
  ...         if m:
  ...             commands.append(m.group(1))
  >>> scommands = list(sorted(commands))
  >>> for i, command in enumerate(scommands):
  ...     if command != commands[i]:
  ...         print('commands in debugcommands.py not sorted; first differing '
  ...               'command is %s; expected %s' % (commands[i], command))
  ...         break

