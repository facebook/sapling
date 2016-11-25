#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ hg locate -X contrib/python-zstandard | sed 's-\\-/-g' |
  >   xargs "$check_code" --warnings --per-file=0 || false
  Skipping hgext/fsmonitor/pywatchman/__init__.py it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/bser.c it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/capabilities.py it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/msc_stdint.h it has no-che?k-code (glob)
  Skipping hgext/fsmonitor/pywatchman/pybser.py it has no-che?k-code (glob)
  Skipping i18n/polib.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping mercurial/statprof.py it has no-che?k-code (glob)

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
