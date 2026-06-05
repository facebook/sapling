#chg-compatible

#require version-control normal-layout no-eden

  $ eagerepo
  $ enable amend undo
  $ cat <<'EOF' > scanhelptopics.py
  > from __future__ import absolute_import, print_function
  > import re
  > import sys
  > if sys.platform == "win32":
  >     import os, msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  > topics = set()
  > topicre = re.compile(r':prog:`help ([a-z0-9\-.]+)`')
  > paths = sys.argv[1:] or [line.strip() for line in sys.stdin]
  > for fname in paths:
  >     with open(fname) as f:
  >         topics.update(m.group(1) for m in topicre.finditer(f.read()))
  > for s in sorted(topics):
  >     print(s)
  > EOF

  $ cd "$TESTDIR"/..

Check if ":prog:`help TOPIC`" is valid:
  $ sl-source-files 'sapling/**/*.py' \
  > | sed 's|\\|/|g' \
  > | $PYTHON "$TESTTMP/scanhelptopics.py" > $TESTTMP/topics
