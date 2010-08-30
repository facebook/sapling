  $ hg init test
  $ cd test

  $ cat > .hg/hgrc <<EOF
  > [extensions]
  > interhg =
  > 
  > [interhg]
  > issues = s|Issue(\d+)|<a href="http://bts.example.org/issue\1">Issue\1</a>|
  > 
  > # yes, 'x' is a weird delimiter...
  > markbugs = sxbugx<i class="\x">bug</i>x
  > EOF

  $ touch foo
  $ hg add foo
  $ hg commit -d '1 0' -m 'Issue123: fixed the bug!'

  $ hg serve -n test -p $HGPORT -d --pid-file=hg.pid -A access.log -E errors.log
  $ cat hg.pid >> $DAEMON_PIDS

log

  $ "$TESTDIR/get-with-headers.py" localhost:$HGPORT '/' | grep bts
    <td class="description"><a href="/rev/1b0e7ece6bd6"><a href="http://bts.example.org/issue123">Issue123</a>: fixed the <i class="x">bug</i>!</a><span class="branchhead">default</span> <span class="tag">tip</span> </td>

errors

  $ cat errors.log
