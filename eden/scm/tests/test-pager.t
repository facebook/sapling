  $ setconfig devel.segmented-changelog-rev-compat=true
  $ setconfig status.use-rust=False workingcopy.use-rust=False
  $ setconfig workingcopy.ruststatus=False
  $ cat >> fakepager.py <<EOF
  > import sys
  > printed = False
  > for line in sys.stdin:
  >     sys.stdout.write('paged! %r\n' % line)
  >     printed = True
  > if not printed:
  >     sys.stdout.write('paged empty output!\n')
  > EOF

Enable ui.assume-tty so that the pager will start, and set the pager to our
fake pager that lets us see when the pager was running.
  $ setconfig ui.assume-tty=yes ui.color=no
  $ cat >>$HGRCPATH <<EOF
  > [pager]
  > pager = $PYTHON $TESTTMP/fakepager.py
  > EOF

  $ hg init repo
  $ cd repo
  $ echo a >> a
  $ hg add a
  $ hg ci -m 'add a'
  $ for x in `seq 1 10`; do
  >   echo a $x >> a
  >   hg ci -m "modify a $x"
  > done

By default diff and log are paged, but id is not:

  $ hg diff -c bce2655495562347d8ef11e2ffe1e0e6f9fdbbf2 --pager=yes
  paged! 'diff -r f4be7687d414 -r bce265549556 a\n'
  paged! '--- a/a\tThu Jan 01 00:00:00 1970 +0000\n'
  paged! '+++ b/a\tThu Jan 01 00:00:00 1970 +0000\n'
  paged! '@@ -1,2 +1,3 @@\n'
  paged! ' a\n'
  paged! ' a 1\n'
  paged! '+a 2\n'

  $ hg log --limit 2
  paged! 'commit:      46106edeeb38\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'commit:      6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

  $ hg id
  46106edeeb38

We can control the pager from the config

  $ hg log --limit 1 --config 'ui.paginate=False'
  commit:      46106edeeb38
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 10
  
  $ hg log --limit 1 --config 'ui.paginate=0'
  commit:      46106edeeb38
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 10
  
  $ hg log --limit 1 --config 'ui.paginate=1'
  paged! 'commit:      46106edeeb38\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'

explicit --pager=on should take precedence over other configurations
(issue5580)

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > paginate = false
  > EOF
  $ hg log --limit 1 --pager=on
  paged! 'commit:      46106edeeb38\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'

  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > # true is default value of ui.paginate
  > paginate = true
  > EOF
  $ hg log --limit 1 --pager=off
  commit:      46106edeeb38
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 10
  

We can enable the pager on id:

BROKEN: should be paged
  $ hg --config pager.attend-id=yes id
  46106edeeb38

Setting attend-$COMMAND to a false value works, even with pager in
core:
  $ hg --config pager.attend-diff=no diff -c bce2655495562347d8ef11e2ffe1e0e6f9fdbbf2
  diff -r f4be7687d414 -r bce265549556 a
  --- a/a	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,2 +1,3 @@
   a
   a 1
  +a 2

Command aliases should have same behavior as main command

  $ hg history --limit 2
  paged! 'commit:      46106edeeb38\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! 'commit:      6dd8ea7dd621\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'

Abbreviated command alias should also be paged

  $ hg history -l 1
  paged! 'commit:      46106edeeb38\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'

Attend for an abbreviated command does not work

  $ hg --config pager.attend-ident=true ident
  46106edeeb38

Pager should not start if stdout is not a tty.

  $ hg log -l1 -q --config ui.assume-tty=no
  46106edeeb38

Pager should be disabled if pager.pager is empty (otherwise the output would
be silently lost.)

  $ hg log -l1 -q --config pager.pager=
  46106edeeb38

Pager with color enabled allows colors to come through by default,
even though stdout is no longer a tty.
  $ cat >> $HGRCPATH <<EOF
  > [ui]
  > color = always
  > [color]
  > mode = ansi
  > EOF
  $ hg log --limit 3
  paged! '\x1b[0m\x1b[1m\x1b[93mcommit:      46106edeeb38\x1b[0m\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 10\n'
  paged! '\n'
  paged! '\x1b[0m\x1b[1m\x1b[93mcommit:      6dd8ea7dd621\x1b[0m\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 9\n'
  paged! '\n'
  paged! '\x1b[0m\x1b[1m\x1b[93mcommit:      cff05a6312fe\x1b[0m\n'
  paged! 'user:        test\n'
  paged! 'date:        Thu Jan 01 00:00:00 1970 +0000\n'
  paged! 'summary:     modify a 8\n'
  paged! '\n'

#if no-chg
An invalid pager command name is reported sensibly if we don't have to
use shell=True in the subprocess call:
  $ hg log --limit 3 --config pager.pager=/this-command-better-never-exist
  missing pager command '*/this-command-better-never-exist', skipping pager (glob)
  \x1b[0m\x1b[1m\x1b[93mcommit:      46106edeeb38\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 10
  
  \x1b[0m\x1b[1m\x1b[93mcommit:      6dd8ea7dd621\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 9
  
  \x1b[0m\x1b[1m\x1b[93mcommit:      cff05a6312fe\x1b[0m (esc)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a 8
  
#endif

A complicated pager command gets worse behavior. Bonus points if you can
improve this.
  $ hg log --limit 3 \
  >   --config pager.pager='this-command-better-never-exist --seriously' \
  >  2>/dev/null || true

Pager works with shell aliases.

  $ cat >> $HGRCPATH <<EOF
  > [alias]
  > echoa = !echo a
  > EOF

  $ hg echoa
  a
BROKEN: should be paged
  $ hg --config pager.attend-echoa=yes echoa
  a

Pager works with hg aliases including environment variables.

  $ cat >> $HGRCPATH <<'EOF'
  > [alias]
  > printa = log -T "$A\n" -r 0
  > EOF

  $ A=1 hg --config pager.attend-printa=yes printa
  paged! '$A\n'
  $ A=2 hg --config pager.attend-printa=yes printa
  paged! '$A\n'

Something that's explicitly attended is still not paginated if the
pager is globally set to off using a flag:
  $ A=2 hg --config pager.attend-printa=yes printa --pager=no
  $A

Pager should not override the exit code of other commands

  $ cat >> $TESTTMP/fortytwo.py <<'EOF'
  > from edenscm import commands, registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('fortytwo', [], 'fortytwo', norepo=True)
  > def fortytwo(ui, *opts):
  >     ui.write('42\n')
  >     return 42
  > EOF

  $ cat >> $HGRCPATH <<'EOF'
  > [extensions]
  > fortytwo = $TESTTMP/fortytwo.py
  > EOF

  $ hg fortytwo --pager=on
  paged! '42\n'
  [42]

A command that asks for paging using ui.pager() directly works:
  $ hg blame --color=no a
  paged! ' 0: a\n'
  paged! ' 1: a 1\n'
  paged! ' 2: a 2\n'
  paged! ' 3: a 3\n'
  paged! ' 4: a 4\n'
  paged! ' 5: a 5\n'
  paged! ' 6: a 6\n'
  paged! ' 7: a 7\n'
  paged! ' 8: a 8\n'
  paged! ' 9: a 9\n'
  paged! '10: a 10\n'
but not with HGPLAIN
  $ HGPLAIN=1 hg blame a
   0: a
   1: a 1
   2: a 2
   3: a 3
   4: a 4
   5: a 5
   6: a 6
   7: a 7
   8: a 8
   9: a 9
  10: a 10
explicit flags work too:
  $ hg blame --pager=no --color=no a
   0: a
   1: a 1
   2: a 2
   3: a 3
   4: a 4
   5: a 5
   6: a 6
   7: a 7
   8: a 8
   9: a 9
  10: a 10

A command with --output option:

  $ hg cat -r'desc(add)' a
  paged! 'a\n'
  $ hg cat -r'desc(add)' a --output=-
  paged! 'a\n'
  $ hg cat -r'desc(add)' a --output=out
  $ rm out

Put annotate in the ignore list for pager:
  $ cat >> $HGRCPATH <<EOF
  > [pager]
  > ignore = annotate
  > EOF
  $ hg blame --color=no a
   0: a
   1: a 1
   2: a 2
   3: a 3
   4: a 4
   5: a 5
   6: a 6
   7: a 7
   8: a 8
   9: a 9
  10: a 10

During pushbuffer, pager should not start:
  $ cat > $TESTTMP/pushbufferpager.py <<EOF
  > def uisetup(ui):
  >     ui.pushbuffer()
  >     ui.pager('mycmd')
  >     ui.write('content\n')
  >     ui.write(ui.popbuffer())
  > EOF

  $ echo append >> a
  $ hg --config extensions.pushbuffer=$TESTTMP/pushbufferpager.py status --color=off
  content
  paged! 'M a\n'

Environment variables like LESS and LV are set automatically:
  $ cat > $TESTTMP/printlesslv.py <<EOF
  > from __future__ import absolute_import
  > import os
  > import sys
  > sys.stdin.read()
  > for name in ['LESS', 'LV']:
  >     sys.stdout.write(('%s=%s\n') % (name, os.environ.get(name, '-')))
  > sys.stdout.flush()
  > EOF

  $ cat >> $HGRCPATH <<EOF
  > [alias]
  > noop = log -r 0 -T ''
  > [ui]
  > formatted=1
  > [pager]
  > pager = $PYTHON $TESTTMP/printlesslv.py
  > EOF
  $ unset LESS
  $ unset LV
  $ hg noop --pager=on
  LESS=FRX
  LV=-c
  $ LESS=EFGH hg noop --pager=on
  LESS=EFGH
  LV=-c
