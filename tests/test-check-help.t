#require test-repo normal-layout

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cat <<'EOF' > scanhelptopics.py
  > from __future__ import absolute_import, print_function
  > import re
  > import sys
  > if sys.platform == "win32":
  >     import os, msvcrt
  >     msvcrt.setmode(sys.stdout.fileno(), os.O_BINARY)
  > topics = set()
  > topicre = re.compile(r':hg:`help ([a-z0-9\-.]+)`')
  > for fname in sys.argv:
  >     with open(fname) as f:
  >         topics.update(m.group(1) for m in topicre.finditer(f.read()))
  > for s in sorted(topics):
  >     print(s)
  > EOF

  $ cd "$TESTDIR"/..

Check if ":hg:`help TOPIC`" is valid:
(use "xargs -n1 -t" to see which help commands are executed)

  $ NPROC=`python -c 'import multiprocessing; print(multiprocessing.cpu_count())'`
  $ testrepohg files 'glob:edenscm/**/*.py' \
  > | sed 's|\\|/|g' \
  > | xargs $PYTHON "$TESTTMP/scanhelptopics.py" > $TESTTMP/topics

Remove subversion from the list; it does not work on macOS and casuses this test
to print errors.
  $ grep -v subversion $TESTTMP/topics > $TESTTMP/topics_filtered
  $ cat $TESTTMP/topics_filtered | xargs -n1 -P $NPROC hg --cwd / help >/dev/null 2>$TESTTMP/results
  $ sort $TESTTMP/results
