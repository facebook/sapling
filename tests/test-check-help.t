#require test-repo

  $ . "$TESTDIR/helpers-testrepo.sh"

  $ cat <<'EOF' > scanhelptopics.py
  > from __future__ import absolute_import, print_function
  > import re
  > import sys
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

  $ hg files 'glob:{hgext,mercurial}/**/*.py' \
  > | xargs python "$TESTTMP/scanhelptopics.py" \
  > | xargs -n1 hg help > /dev/null
