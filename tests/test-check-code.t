#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"
  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ hg locate -X contrib/python-zstandard -X hgext/fsmonitor/pywatchman |
  > sed 's-\\-/-g' | xargs "$check_code" --warnings --per-file=0 || false
  Skipping i18n/polib.py it has no-che?k-code (glob)
  mercurial/demandimport.py:312:
   >     if os.environ.get('HGDEMANDIMPORT') != 'disable':
   use encoding.environ instead (py3)
  mercurial/encoding.py:54:
   >     environ = os.environ
   use encoding.environ instead (py3)
  mercurial/encoding.py:56:
   >     environ = os.environb
   use encoding.environ instead (py3)
  mercurial/encoding.py:61:
   >                    for k, v in os.environ.items())
   use encoding.environ instead (py3)
  mercurial/encoding.py:203:
   >                    for k, v in os.environ.items())
   use encoding.environ instead (py3)
  Skipping mercurial/httpclient/__init__.py it has no-che?k-code (glob)
  Skipping mercurial/httpclient/_readers.py it has no-che?k-code (glob)
  mercurial/policy.py:46:
   >     if 'HGMODULEPOLICY' in os.environ:
   use encoding.environ instead (py3)
  mercurial/policy.py:47:
   >         policy = os.environ['HGMODULEPOLICY'].encode('utf-8')
   use encoding.environ instead (py3)
  mercurial/policy.py:49:
   >     policy = os.environ.get('HGMODULEPOLICY', policy)
   use encoding.environ instead (py3)
  Skipping mercurial/statprof.py it has no-che?k-code (glob)
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
