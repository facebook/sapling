#chg-compatible

  $ setconfig status.use-rust=True workingcopy.use-rust=True
  $ setconfig workingcopy.ruststatus=False
  $ configure modernclient
  $ setconfig ui.color=always color.mode=ansi
Terminfo codes compatibility fix
  $ setconfig color.color.none=0

  $ newclientrepo repo1
  $ mkdir a b a/1 b/1 b/2
  $ touch in_root a/in_a b/in_b a/1/in_a_1 b/1/in_b_1 b/2/in_b_2

hg status in repo root:

  $ hg status
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? a/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? a/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? b/1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? b/2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? b/in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_root\x1b[0m (esc)

  $ hg status --color=debug
  [? a/1/in_a_1|status.unknown]
  [? a/in_a|status.unknown]
  [? b/1/in_b_1|status.unknown]
  [? b/2/in_b_2|status.unknown]
  [? b/in_b|status.unknown]
  [? in_root|status.unknown]
HGPLAIN disables color
  $ HGPLAIN=1 hg status --color=debug
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
HGPLAINEXCEPT=color does not disable color
  $ HGPLAINEXCEPT=color hg status --color=debug
  [? a/1/in_a_1|status.unknown]
  [? a/in_a|status.unknown]
  [? b/1/in_b_1|status.unknown]
  [? b/2/in_b_2|status.unknown]
  [? b/in_b|status.unknown]
  [? in_root|status.unknown]

hg status with template
  $ hg status -T "{label('red', path)}\n" --color=debug
  [red|a/1/in_a_1]
  [red|a/in_a]
  [red|b/1/in_b_1]
  [red|b/2/in_b_2]
  [red|b/in_b]
  [red|in_root]

hg status . in repo root:

  $ hg status .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35ma/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35ma/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35mb/1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35mb/2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35mb/in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_root\x1b[0m (esc)

  $ hg status --cwd a
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? 1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../b/1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../b/2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../b/in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../in_root\x1b[0m (esc)
  $ hg status --cwd a .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_a\x1b[0m (esc)
  $ hg status --cwd a ..
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../b/1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../b/2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../b/in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../in_root\x1b[0m (esc)

  $ hg status --cwd b
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../a/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../a/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? 1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? 2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../in_root\x1b[0m (esc)
  $ hg status --cwd b .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b\x1b[0m (esc)
  $ hg status --cwd b ..
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../a/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../a/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../in_root\x1b[0m (esc)

  $ hg status --cwd a/1
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../b/1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../b/2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../b/in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../in_root\x1b[0m (esc)
  $ hg status --cwd a/1 .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_a_1\x1b[0m (esc)
  $ hg status --cwd a/1 ..
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../in_a\x1b[0m (esc)

  $ hg status --cwd b/1
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../a/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../a/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../in_root\x1b[0m (esc)
  $ hg status --cwd b/1 .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b_1\x1b[0m (esc)
  $ hg status --cwd b/1 ..
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../2/in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../in_b\x1b[0m (esc)

  $ hg status --cwd b/2
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../a/1/in_a_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../a/in_a\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? in_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../in_b\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? ../../in_root\x1b[0m (esc)
  $ hg status --cwd b/2 .
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b_2\x1b[0m (esc)
  $ hg status --cwd b/2 ..
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../1/in_b_1\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35min_b_2\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35m../in_b\x1b[0m (esc)

Make sure --color=never works
  $ hg status --color=never
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

Make sure ui.formatted=False works
  $ hg status --color=auto --config ui.formatted=False
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

  $ newclientrepo repo2
  $ touch modified removed deleted ignored
  $ echo "ignored" > .gitignore
  $ hg ci -A -m 'initial checkin'
  adding .gitignore
  adding deleted
  adding modified
  adding removed
  $ hg log --color=debug
  [log.changeset changeset.draft|commit:      51a28a6611a2]
  [log.user|user:        test]
  [log.date|date:        Thu Jan 01 00:00:00 1970 +0000]
  [log.summary|summary:     initial checkin]
  
  $ hg log -Tcompact --color=debug
     [log.node|51a28a6611a2]   [log.date|1970-01-01 00:00 +0000]   [log.user|test]
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

  $ hg status
  \x1b[0m\x1b[1m\x1b[32mA added\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[31mR removed\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[36m! deleted\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? unknown\x1b[0m (esc)

hg status modified added removed deleted unknown never-existed ignored:

  $ hg status modified added removed deleted unknown never-existed ignored
  never-existed: * (glob)
  \x1b[0m\x1b[1m\x1b[32mA \x1b[0m\x1b[0m\x1b[1m\x1b[32madded\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[31mR \x1b[0m\x1b[0m\x1b[1m\x1b[31mremoved\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[36m! \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[36mdeleted\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35munknown\x1b[0m (esc)

  $ hg copy modified copied

hg status -C:

  $ hg status -C
  \x1b[0m\x1b[1m\x1b[32mA added\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32mA copied\x1b[0m (esc)
    modified
  \x1b[0m\x1b[1m\x1b[31mR removed\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[36m! deleted\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? unknown\x1b[0m (esc)

hg status -A:

  $ hg status -A
  \x1b[0m\x1b[1m\x1b[32mA \x1b[0m\x1b[0m\x1b[1m\x1b[32madded\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32mA \x1b[0m\x1b[0m\x1b[1m\x1b[32mcopied\x1b[0m (esc)
    modified
  \x1b[0m\x1b[1m\x1b[31mR \x1b[0m\x1b[0m\x1b[1m\x1b[31mremoved\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[36m! \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[36mdeleted\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? \x1b[0m\x1b[0m\x1b[1m\x1b[4m\x1b[35munknown\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[30mI \x1b[0m\x1b[0m\x1b[1m\x1b[30mignored\x1b[0m (esc)
  C .gitignore
  C modified


  $ echo "ignoreddir" > .gitignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

hg status ignoreddir/file:

  $ hg status ignoreddir/file

hg status -i ignoreddir/file:

  $ hg status -i ignoreddir/file
  \x1b[0m\x1b[1m\x1b[30mI \x1b[0m\x1b[0m\x1b[1m\x1b[30mignoreddir/file\x1b[0m (esc)
  $ cd ..

check 'status -q' and some combinations

  $ newclientrepo repo3
  $ touch modified removed deleted ignored
  $ echo "ignored" > .gitignore
  $ hg commit -A -m 'initial checkin'
  adding .gitignore
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

  $ EDENSCM_LOG=termstyle=warn hg --config color.status.modified=periwinkle status
   WARN termstyle::effects: unknown style effect effect="periwinkle"
   WARN termstyle::effects: couldn't apply style spec spec="periwinkle"
  M modified
  \x1b[0m\x1b[1m\x1b[32mA added\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32mA copied\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[31mR removed\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[36m! deleted\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[4m\x1b[35m? unknown\x1b[0m (esc)

Run status with 2 different flags.
Check if result is the same or different.
If result is not as expected, raise error

  $ assert() {
  >     hg status $1 > ../a
  >     hg status $2 > ../b
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

  $ newclientrepo repo4
  $ echo "file a" > a
  $ echo "file b" > b
  $ hg add a b
  $ hg commit -m "initial"
  $ echo "file a change 1" > a
  $ echo "file b change 1" > b
  $ hg commit -m "head 1"
  $ hg goto 'desc(initial)'
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo "file a change 2" > a
  $ echo "file b change 2" > b
  $ hg commit -m "head 2"
  $ hg merge
  merging a
  merging b
  warning: 1 conflicts while merging a! (edit, then use 'hg resolve --mark')
  warning: 1 conflicts while merging b! (edit, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg goto -C .' to abandon
  [1]
  $ hg resolve -m b

hg resolve with one unresolved, one resolved:

  $ hg resolve -l
  \x1b[0m\x1b[1m\x1b[31mU \x1b[0m\x1b[0m\x1b[1m\x1b[31ma\x1b[0m (esc)
  \x1b[0m\x1b[1m\x1b[32mR \x1b[0m\x1b[0m\x1b[1m\x1b[32mb\x1b[0m (esc)

color coding of error message with current availability of curses

  $ hg unknowncommand > /dev/null
  unknown command 'unknowncommand'
  (use 'hg help' to get help)
  [255]

color coding of error message without curses

  $ echo 'raise ImportError' > curses.py
  $ PYTHONPATH=`pwd`:$PYTHONPATH hg unknowncommand > /dev/null
  unknown command 'unknowncommand'
  (use 'hg help' to get help)
  [255]

  $ cd ..
