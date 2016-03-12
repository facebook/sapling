  $ cat >> fakepager.py <<EOF
  > import sys
  > for line in sys.stdin:
  >     sys.stdout.write('paged! %r\n' % line)
  > EOF

Enable ui.formatted because pager won't fire without it, and set up
pager and tell it to use our fake pager that lets us see when the
pager was running.
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > formatted = yes
  > [extensions]
  > pager=
  > [pager]
  > pager = python $TESTTMP/fakepager.py
  > EOF

  $ hg init repo
  $ cd repo
  $ echo a >> a
  $ hg add a
  $ hg ci -m 'add a'
  $ for x in `python $TESTDIR/seq.py 1 10`; do
  >   echo a $x >> a
  >   hg ci -m "modify a $x"
  > done

By default diff and log are paged, but summary is not:

  $ hg diff -c 2 --pager=yes
  paged! 'diff -r f4be7687d414 -r bce265549556 a\n'
  paged! '--- a/a\tThu Jan 01 00:00:00 1970 +0000\n'
  paged! '+++ b/a\tThu Jan 01 00:00:00 1970 +0000\n'
  paged! '@@ -1,2 +1,3 @@\n'
  paged! ' a\n'
  paged! ' a 1\n'
  paged! '+a 2\n'

  $ hg log --limit 2
  paged! 'changeset:   10:46106edeeb38\n'
  paged! 'tag:         tip\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'changeset:   9:6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

  $ hg summary
  parent: 10:46106edeeb38 tip
   modify a 10
  branch: default
  commit: (clean)
  update: (current)
  phases: 11 draft

We can enable the pager on summary:

  $ hg --config pager.attend-summary=yes summary
  paged! 'parent: 10:46106edeeb38 tip\n'
  paged! ' modify a 10\n'
  paged! 'branch: default\n'
  paged! 'commit: (clean)\n'
  paged! 'update: (current)\n'
  paged! 'phases: 11 draft\n'

If we completely change the attend list that's respected:

  $ hg --config pager.attend-diff=no diff -c 2
  diff -r f4be7687d414 -r bce265549556 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   a
   a 1
  +a 2

  $ hg --config pager.attend=summary diff -c 2
  diff -r f4be7687d414 -r bce265549556 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   a
   a 1
  +a 2

If 'log' is in attend, then 'history' should also be paged:
  $ hg history --limit 2 --config pager.attend=log
  paged! 'changeset:   10:46106edeeb38\n'
  paged! 'tag:         tip\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'changeset:   9:6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

Possible bug: history is explicitly ignored in pager config, but
because log is in the attend list it still gets pager treatment.

  $ hg history --limit 2 --config pager.attend=log \
  >   --config pager.ignore=history
  paged! 'changeset:   10:46106edeeb38\n'
  paged! 'tag:         tip\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'changeset:   9:6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

Possible bug: history is explicitly marked as attend-history=no, but
it doesn't fail to get paged because log is still in the attend list.

  $ hg history --limit 2 --config pager.attend-history=no
  paged! 'changeset:   10:46106edeeb38\n'
  paged! 'tag:         tip\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'changeset:   9:6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

Possible bug: disabling pager for log but enabling it for history
doesn't result in history being paged.

  $ hg history --limit 2 --config pager.attend-log=no \
  > --config pager.attend-history=yes
  changeset:   10:46106edeeb38
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 10
  
  changeset:   9:6dd8ea7dd621
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 9
  

Pager with color enabled allows colors to come through by default,
even though stdout is no longer a tty.
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > color=
  > [color]
  > mode = ansi
  > EOF
  $ hg log --limit 3
  paged! '\x1b[0;33mchangeset:   10:46106edeeb38\x1b[0m\n'
  paged! 'tag:         tip\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! '\x1b[0;33mchangeset:   9:6dd8ea7dd621\x1b[0m\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'
  paged! '\x1b[0;33mchangeset:   8:cff05a6312fe\x1b[0m\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 8\n'
  paged! '\n'
