#chg-compatible

#require test-repo

  $ eagerepo
  $ . "$TESTDIR/helpers-testrepo.sh"
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ testrepohg files . | grep -Ev "^(sapling/ext/extlib/pywatchman|lib/cdatapack|lib/third-party|sapling/thirdparty|fb|newdoc|tests|sapling/templates/static|i18n|slides|.*\\.(bin|bindag|hg|pdf|jpg)$)" \
  > | sed 's-\\-/-g' > $TESTTMP/files.txt

  $ NPROC=`hg debugpython -- -c 'import multiprocessing; print(str(multiprocessing.cpu_count()))'`
  $ cat $TESTTMP/files.txt | PYTHONPATH= xargs -n64 -P $NPROC contrib/check-code.py --warnings --per-file=0 | LC_ALL=C sort
  Skipping sapling/commands/eden.py it has no-che?k-code (glob)
  Skipping sapling/ext/globalrevs.py it has no-che?k-code (glob)
  Skipping sapling/ext/hgsql.py it has no-che?k-code (glob)
  Skipping sapling/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping sapling/httpclient/_readers.py it has no-che?k-code (glob)
  Skipping sapling/statprof.py it has no-che?k-code (glob)

@commands in debugcommands.py should be in alphabetical order.

  >>> import re
  >>> commands = []
  >>> with open('sapling/commands/debug.py', 'rb') as fh:
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

