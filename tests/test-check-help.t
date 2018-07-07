#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > hgsubversion=
  > EOF

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
  $ testrepohg files 'glob:{hgdemandimport,hgext,mercurial}/**/*.py' \
  > | sed 's|\\|/|g' \
  > | xargs $PYTHON "$TESTTMP/scanhelptopics.py" \
  > | xargs -n1 -P $NPROC hg --cwd / help >/dev/null 2>$TESTTMP/results
  $ cat $TESTTMP/results | sort
