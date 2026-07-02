
#require version-control no-eden

  $ eagerepo
  $ cd "$TESTDIR"/..
  warning: no longer inside TESTTMP

New errors are not allowed. Warnings are strongly discouraged.
(The writing "no-che?k-code" is for not skipping this file when checking.)

  $ sl-source-files '**' | $PYTHON -c 'import re, sys; exclude = re.compile(r"^(build\.py|lib/virtual-repo/virtual-tree/src/serialized/create_example\.py|sapling/ext/extlib/pywatchman|lib/cdatapack|lib/third-party|sapling/thirdparty|fb|newdoc|tests|sapling/templates/static|i18n|slides|.*\.(bin|bindag|hg|pdf|jpg)$)"); [sys.stdout.write(path) for path in sys.stdin if not exclude.search(path.rstrip("\n").replace("\\", "/"))]' \
  > | sed 's-\\-/-g' > $TESTTMP/files.txt

  $ PYTHONPATH= $PYTHON << EOF | LC_ALL=C sort
  > import os
  > import subprocess
  > import sys
  > files = open(os.path.join(os.environ["TESTTMP"], "files.txt")).read().splitlines()
  > env = os.environ.copy()
  > env["PYTHONPATH"] = ""
  > for i in range(0, len(files), 64):
  >     subprocess.run(
  >         [sys.executable, "contrib/check-code.py", "--warnings", "--per-file=0"] + files[i : i + 64],
  >         check=True,
  >         env=env,
  >     )
  > EOF
  Skipping sapling/commands/eden.py it has no-che?k-code (glob)
  Skipping sapling/ext/globalrevs.py it has no-che?k-code (glob)
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
