  $ check_code="$TESTDIR"/../contrib/check-code.py
  $ cd "$TESTDIR"/..
  $ if hg identify -q > /dev/null; then :
  > else
  >     echo "skipped: not a Mercurial working dir" >&2
  >     exit 80
  > fi

New errors are not allowed. Warnings are strongly discouraged.

  $ hg manifest | xargs "$check_code" --warnings --nolineno --per-file=0 \
  > || false
  tests/test-hgweb-raw.t:0:
   >   $ while kill `cat hg.pid` 2>/dev/null; do sleep 0; done
   don't use kill, use killdaemons.py
   don't use kill, use killdaemons.py
  tests/test-https.t:0:
   >   $ while kill `cat hg1.pid` 2>/dev/null; do sleep 0; done
   don't use kill, use killdaemons.py
  tests/test-inotify-debuginotify.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify-issue1371.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify-issue1542.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify-issue1556.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify-lookup.t:0:
   >   $ kill `cat .hg/inotify.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify.t:0:
   >   $ kill `cat ../hg2.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-inotify.t:0:
   >   $ kill `cat hg3.pid`
   don't use kill, use killdaemons.py
  tests/test-obsolete.t:0:
   >   $ kill `cat hg.pid`
   don't use kill, use killdaemons.py
   don't use kill, use killdaemons.py
  tests/test-serve.t:0:
   >   >        kill `cat hg.pid`
   don't use kill, use killdaemons.py
  tests/test-serve.t:0:
   >   >        kill `cat hg.pid` 2>/dev/null
   don't use kill, use killdaemons.py
  [1]
