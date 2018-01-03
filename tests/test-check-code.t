#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg locate \
  > -X contrib/python-zstandard \
  > -X hgext/fsmonitor/pywatchman \
  > -X mercurial/thirdparty \
  > -X fb-hgext \
  > | sed 's-\\-/-g' | "$check_code" --warnings --per-file=0 - || false
  Skipping hgsql/hgsql.py it has no-che?k-code (glob)
  Skipping hgsql/tests/heredoctest.py it has no-che?k-code (glob)
  Skipping hgsql/tests/killdaemons.py it has no-che?k-code (glob)
  Skipping hgsql/tests/run-tests.py.old it has no-che?k-code (glob)
  Skipping hgsql/tests/test-encoding.t it has no-che?k-code (glob)
  Skipping hgsql/tests/test-race-conditions.t it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping mercurial/statprof.py it has no-che?k-code (glob)
  Skipping tests/badserverext.py it has no-che?k-code (glob)
  tests/test-remotenames-basic.t:308:
   >   $ hg help bookmarks  | grep -A 3 -- '--track'
   don't use grep's context flags
  [1]

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('mercurial/debugcommands.py', 'rb') as fh:
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

Prevent adding new files in the root directory accidentally.

  $ testrepohg files 'glob:*'
  .arcconfig
  .clang-format
  .editorconfig
  .hgignore
  .hgsigs
  .hgtags
  .jshintrc
  CONTRIBUTING
  CONTRIBUTORS
  COPYING
  Makefile
  README.rst
  hg
  hgeditor
  hgweb.cgi
  setup.py
