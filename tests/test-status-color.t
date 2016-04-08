  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > color =
  > [color]
  > mode = ansi
  > EOF
Terminfo codes compatibility fix
  $ echo "color.none=0" >> $HGRCPATH

  $ hg init repo1
  $ cd repo1
  $ mkdir a b a/1 b/1 b/2
  $ touch in_root a/in_a b/in_b a/1/in_a_1 b/1/in_b_1 b/2/in_b_2

hg status in repo root:

  $ hg status --color=always
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)

  $ hg status --color=debug
  [status.unknown|? ][status.unknown|a/1/in_a_1]
  [status.unknown|? ][status.unknown|a/in_a]
  [status.unknown|? ][status.unknown|b/1/in_b_1]
  [status.unknown|? ][status.unknown|b/2/in_b_2]
  [status.unknown|? ][status.unknown|b/in_b]
  [status.unknown|? ][status.unknown|in_root]

hg status with template
  $ hg status -T "{label('red', path)}\n" --color=debug
  [red|a/1/in_a_1]
  [red|a/in_a]
  [red|b/1/in_b_1]
  [red|b/2/in_b_2]
  [red|b/in_b]
  [red|in_root]

hg status . in repo root:

  $ hg status --color=always .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)

  $ hg status --color=always --cwd a
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)
  $ hg status --color=always --cwd a .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_a\x1b[0m (esc)
  $ hg status --color=always --cwd a ..
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../b/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../b/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../b/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../in_root\x1b[0m (esc)

  $ hg status --color=always --cwd b
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)
  $ hg status --color=always --cwd b .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b\x1b[0m (esc)
  $ hg status --color=always --cwd b ..
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../a/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../a/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../in_root\x1b[0m (esc)

  $ hg status --color=always --cwd a/1
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)
  $ hg status --color=always --cwd a/1 .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_a_1\x1b[0m (esc)
  $ hg status --color=always --cwd a/1 ..
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../in_a\x1b[0m (esc)

  $ hg status --color=always --cwd b/1
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)
  $ hg status --color=always --cwd b/1 .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b_1\x1b[0m (esc)
  $ hg status --color=always --cwd b/1 ..
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../in_b\x1b[0m (esc)

  $ hg status --color=always --cwd b/2
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/1/in_a_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4ma/in_a\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/2/in_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4mb/in_b\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_root\x1b[0m (esc)
  $ hg status --color=always --cwd b/2 .
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b_2\x1b[0m (esc)
  $ hg status --color=always --cwd b/2 ..
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../1/in_b_1\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4min_b_2\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4m../in_b\x1b[0m (esc)

Make sure --color=never works
  $ hg status --color=never
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

Make sure ui.formatted=False works
  $ hg status --config ui.formatted=False
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

  $ cd ..

  $ hg init repo2
  $ cd repo2
  $ touch modified removed deleted ignored
  $ echo "^ignored$" > .hgignore
  $ hg ci -A -m 'initial checkin'
  adding .hgignore
  adding deleted
  adding modified
  adding removed
  $ hg log --color=debug
  [log.changeset changeset.draft|changeset:   0:389aef86a55e]
  [log.tag|tag:         tip]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     initial checkin]
  
  $ hg log -Tcompact --color=debug
  [log.changeset changeset.draft|0][tip]   [log.node|389aef86a55e]   [log.date|1970-01-01 00:00 +0000]   [log.user|test]
    [ui.note log.description|initial checkin]
  
Labels on empty strings should not be displayed, labels on custom
templates should be.

  $ hg log --color=debug -T '{label("my.label",author)}\n{label("skipped.label","")}'
  [my.label|test]
  $ touch modified added unknown ignored
  $ hg add added
  $ hg remove removed
  $ rm deleted

hg status:

  $ hg status --color=always
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1madded\x1b[0m (esc)
  \x1b[0;31;1mR \x1b[0m\x1b[0;31;1mremoved\x1b[0m (esc)
  \x1b[0;36;1;4m! \x1b[0m\x1b[0;36;1;4mdeleted\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4munknown\x1b[0m (esc)

hg status modified added removed deleted unknown never-existed ignored:

  $ hg status --color=always modified added removed deleted unknown never-existed ignored
  never-existed: * (glob)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1madded\x1b[0m (esc)
  \x1b[0;31;1mR \x1b[0m\x1b[0;31;1mremoved\x1b[0m (esc)
  \x1b[0;36;1;4m! \x1b[0m\x1b[0;36;1;4mdeleted\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4munknown\x1b[0m (esc)

  $ hg copy modified copied

hg status -C:

  $ hg status --color=always -C
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1madded\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mcopied\x1b[0m (esc)
  \x1b[0;0m  modified\x1b[0m (esc)
  \x1b[0;31;1mR \x1b[0m\x1b[0;31;1mremoved\x1b[0m (esc)
  \x1b[0;36;1;4m! \x1b[0m\x1b[0;36;1;4mdeleted\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4munknown\x1b[0m (esc)

hg status -A:

  $ hg status --color=always -A
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1madded\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mcopied\x1b[0m (esc)
  \x1b[0;0m  modified\x1b[0m (esc)
  \x1b[0;31;1mR \x1b[0m\x1b[0;31;1mremoved\x1b[0m (esc)
  \x1b[0;36;1;4m! \x1b[0m\x1b[0;36;1;4mdeleted\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4munknown\x1b[0m (esc)
  \x1b[0;30;1mI \x1b[0m\x1b[0;30;1mignored\x1b[0m (esc)
  \x1b[0;0mC \x1b[0m\x1b[0;0m.hgignore\x1b[0m (esc)
  \x1b[0;0mC \x1b[0m\x1b[0;0mmodified\x1b[0m (esc)


hg status -A (with terminfo color):

#if tic

  $ mkdir "$TESTTMP/terminfo"
  $ TERMINFO="$TESTTMP/terminfo" tic "$TESTDIR/hgterm.ti"
  $ TERM=hgterm TERMINFO="$TESTTMP/terminfo" hg status --config color.mode=terminfo --color=always -A
  \x1b[30m\x1b[32m\x1b[1mA \x1b[30m\x1b[30m\x1b[32m\x1b[1madded\x1b[30m (esc)
  \x1b[30m\x1b[32m\x1b[1mA \x1b[30m\x1b[30m\x1b[32m\x1b[1mcopied\x1b[30m (esc)
  \x1b[30m\x1b[30m  modified\x1b[30m (esc)
  \x1b[30m\x1b[31m\x1b[1mR \x1b[30m\x1b[30m\x1b[31m\x1b[1mremoved\x1b[30m (esc)
  \x1b[30m\x1b[36m\x1b[1m\x1b[4m! \x1b[30m\x1b[30m\x1b[36m\x1b[1m\x1b[4mdeleted\x1b[30m (esc)
  \x1b[30m\x1b[35m\x1b[1m\x1b[4m? \x1b[30m\x1b[30m\x1b[35m\x1b[1m\x1b[4munknown\x1b[30m (esc)
  \x1b[30m\x1b[30m\x1b[1mI \x1b[30m\x1b[30m\x1b[30m\x1b[1mignored\x1b[30m (esc)
  \x1b[30m\x1b[30mC \x1b[30m\x1b[30m\x1b[30m.hgignore\x1b[30m (esc)
  \x1b[30m\x1b[30mC \x1b[30m\x1b[30m\x1b[30mmodified\x1b[30m (esc)

#endif


  $ echo "^ignoreddir$" > .hgignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

hg status ignoreddir/file:

  $ hg status --color=always ignoreddir/file

hg status -i ignoreddir/file:

  $ hg status --color=always -i ignoreddir/file
  \x1b[0;30;1mI \x1b[0m\x1b[0;30;1mignoreddir/file\x1b[0m (esc)
  $ cd ..

check 'status -q' and some combinations

  $ hg init repo3
  $ cd repo3
  $ touch modified removed deleted ignored
  $ echo "^ignored$" > .hgignore
  $ hg commit -A -m 'initial checkin'
  adding .hgignore
  adding deleted
  adding modified
  adding removed
  $ touch added unknown ignored
  $ hg add added
  $ echo "test" >> modified
  $ hg remove removed
  $ rm deleted
  $ hg copy modified copied

test unknown color

  $ hg --config color.status.modified=periwinkle status --color=always
  ignoring unknown color/effect 'periwinkle' (configured in color.status.modified)
  M modified
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1madded\x1b[0m (esc)
  \x1b[0;32;1mA \x1b[0m\x1b[0;32;1mcopied\x1b[0m (esc)
  \x1b[0;31;1mR \x1b[0m\x1b[0;31;1mremoved\x1b[0m (esc)
  \x1b[0;36;1;4m! \x1b[0m\x1b[0;36;1;4mdeleted\x1b[0m (esc)
  \x1b[0;35;1;4m? \x1b[0m\x1b[0;35;1;4munknown\x1b[0m (esc)

Run status with 2 different flags.
Check if result is the same or different.
If result is not as expected, raise error

  $ assert() {
  >     hg status --color=always $1 > ../a
  >     hg status --color=always $2 > ../b
  >     if diff ../a ../b > /dev/null; then
  >         out=0
  >     else
  >         out=1
  >     fi
  >     if [ $3 -eq 0 ]; then
  >         df="same"
  >     else
  >         df="different"
  >     fi
  >     if [ $out -ne $3 ]; then
  >         echo "Error on $1 and $2, should be $df."
  >     fi
  > }

assert flag1 flag2 [0-same | 1-different]

  $ assert "-q" "-mard"      0
  $ assert "-A" "-marduicC"  0
  $ assert "-qA" "-mardcC"   0
  $ assert "-qAui" "-A"      0
  $ assert "-qAu" "-marducC" 0
  $ assert "-qAi" "-mardicC" 0
  $ assert "-qu" "-u"        0
  $ assert "-q" "-u"         1
  $ assert "-m" "-a"         1
  $ assert "-r" "-d"         1
  $ cd ..

test 'resolve -l'

  $ hg init repo4
  $ cd repo4
  $ echo "file a" > a
  $ echo "file b" > b
  $ hg add a b
  $ hg commit -m "initial"
  $ echo "file a change 1" > a
  $ echo "file b change 1" > b
  $ hg commit -m "head 1"
  $ hg update 0
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "file a change 2" > a
  $ echo "file b change 2" > b
  $ hg commit -m "head 2"
  created new head
  $ hg merge
  merging a
  merging b
  warning: conflicts while merging a! (edit, then use 'hg resolve --mark')
  warning: conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg resolve -m b

hg resolve with one unresolved, one resolved:

  $ hg resolve --color=always -l
  \x1b[0;31;1mU \x1b[0m\x1b[0;31;1ma\x1b[0m (esc)
  \x1b[0;32;1mR \x1b[0m\x1b[0;32;1mb\x1b[0m (esc)

color coding of error message with current availability of curses

  $ hg unknowncommand > /dev/null
  hg: unknown command 'unknowncommand'
  [255]

color coding of error message without curses

  $ echo 'raise ImportError' > curses.py
  $ PYTHONPATH=`pwd`:$PYTHONPATH hg unknowncommand > /dev/null
  hg: unknown command 'unknowncommand'
  [255]

  $ cd ..
