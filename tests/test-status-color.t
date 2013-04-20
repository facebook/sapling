  $ "$TESTDIR/hghave" tic || exit 80

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "mode=ansi" >> $HGRCPATH
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
  warning: conflicts during merge.
  merging a incomplete! (edit conflicts, then use 'hg resolve --mark')
  merging b
  warning: conflicts during merge.
  merging b incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]
  $ hg resolve -m b

hg resolve with one unresolved, one resolved:

  $ hg resolve --color=always -l
  \x1b[0;31;1mU a\x1b[0m (esc)
  \x1b[0;32;1mR b\x1b[0m (esc)

  $ cd ..
