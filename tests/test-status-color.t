  $ echo "[extensions]" >> $HGRCPATH
  $ echo "color=" >> $HGRCPATH
  $ echo "[color]" >> $HGRCPATH
  $ echo "mode=ansi" >> $HGRCPATH

  $ hg init repo1
  $ cd repo1
  $ mkdir a b a/1 b/1 b/2
  $ touch in_root a/in_a b/in_b a/1/in_a_1 b/1/in_b_1 b/2/in_b_2

hg status in repo root:

  $ hg status --color=always
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m

hg status . in repo root:

  $ hg status --color=always .
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m

  $ hg status --color=always --cwd a
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m
  $ hg status --color=always --cwd a .
  [0;35;1;4m? 1/in_a_1[0m
  [0;35;1;4m? in_a[0m
  $ hg status --color=always --cwd a ..
  [0;35;1;4m? 1/in_a_1[0m
  [0;35;1;4m? in_a[0m
  [0;35;1;4m? ../b/1/in_b_1[0m
  [0;35;1;4m? ../b/2/in_b_2[0m
  [0;35;1;4m? ../b/in_b[0m
  [0;35;1;4m? ../in_root[0m

  $ hg status --color=always --cwd b
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m
  $ hg status --color=always --cwd b .
  [0;35;1;4m? 1/in_b_1[0m
  [0;35;1;4m? 2/in_b_2[0m
  [0;35;1;4m? in_b[0m
  $ hg status --color=always --cwd b ..
  [0;35;1;4m? ../a/1/in_a_1[0m
  [0;35;1;4m? ../a/in_a[0m
  [0;35;1;4m? 1/in_b_1[0m
  [0;35;1;4m? 2/in_b_2[0m
  [0;35;1;4m? in_b[0m
  [0;35;1;4m? ../in_root[0m

  $ hg status --color=always --cwd a/1
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m
  $ hg status --color=always --cwd a/1 .
  [0;35;1;4m? in_a_1[0m
  $ hg status --color=always --cwd a/1 ..
  [0;35;1;4m? in_a_1[0m
  [0;35;1;4m? ../in_a[0m

  $ hg status --color=always --cwd b/1
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m
  $ hg status --color=always --cwd b/1 .
  [0;35;1;4m? in_b_1[0m
  $ hg status --color=always --cwd b/1 ..
  [0;35;1;4m? in_b_1[0m
  [0;35;1;4m? ../2/in_b_2[0m
  [0;35;1;4m? ../in_b[0m

  $ hg status --color=always --cwd b/2
  [0;35;1;4m? a/1/in_a_1[0m
  [0;35;1;4m? a/in_a[0m
  [0;35;1;4m? b/1/in_b_1[0m
  [0;35;1;4m? b/2/in_b_2[0m
  [0;35;1;4m? b/in_b[0m
  [0;35;1;4m? in_root[0m
  $ hg status --color=always --cwd b/2 .
  [0;35;1;4m? in_b_2[0m
  $ hg status --color=always --cwd b/2 ..
  [0;35;1;4m? ../1/in_b_1[0m
  [0;35;1;4m? in_b_2[0m
  [0;35;1;4m? ../in_b[0m
  $ cd ..

  $ hg init repo2
  $ cd repo2
  $ touch modified removed deleted ignored
  $ echo "^ignored$" > .hgignore
  $ hg ci -A -m 'initial checkin' -d "1000000 0"
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
  [0;32;1mA added[0m
  [0;31;1mR removed[0m
  [0;36;1;4m! deleted[0m
  [0;35;1;4m? unknown[0m

hg status modified added removed deleted unknown never-existed ignored:

  $ hg status --color=always modified added removed deleted unknown never-existed ignored
  never-existed: No such file or directory
  [0;32;1mA added[0m
  [0;31;1mR removed[0m
  [0;36;1;4m! deleted[0m
  [0;35;1;4m? unknown[0m

  $ hg copy modified copied

hg status -C:

  $ hg status --color=always -C
  [0;32;1mA added[0m
  [0;32;1mA copied[0m
  [0;0m  modified[0m
  [0;31;1mR removed[0m
  [0;36;1;4m! deleted[0m
  [0;35;1;4m? unknown[0m

hg status -A:

  $ hg status --color=always -A
  [0;32;1mA added[0m
  [0;32;1mA copied[0m
  [0;0m  modified[0m
  [0;31;1mR removed[0m
  [0;36;1;4m! deleted[0m
  [0;35;1;4m? unknown[0m
  [0;30;1mI ignored[0m
  [0;0mC .hgignore[0m
  [0;0mC modified[0m


  $ echo "^ignoreddir$" > .hgignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

hg status ignoreddir/file:

  $ hg status --color=always ignoreddir/file

hg status -i ignoreddir/file:

  $ hg status --color=always -i ignoreddir/file
  [0;30;1mI ignoreddir/file[0m
  $ cd ..

# check 'status -q' and some combinations

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
  [0;32;1mA added[0m
  [0;32;1mA copied[0m
  [0;31;1mR removed[0m
  [0;36;1;4m! deleted[0m
  [0;35;1;4m? unknown[0m

# Run status with 2 different flags.
# Check if result is the same or different.
# If result is not as expected, raise error
  $ assert() {
  >     hg status --color=always $1 > ../a
  >     hg status --color=always $2 > ../b
  >     out=`diff ../a ../b`
  >     if [ $? -ne 0 ]; then
  >         out=1
  >     else
  >         out=0
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

# assert flag1 flag2 [0-same | 1-different]

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

# test 'resolve -l'
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
  merging a failed!
  merging b
  warning: conflicts during merge.
  merging b failed!
  0 files updated, 0 files merged, 0 files removed, 2 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C' to abandon
  $ hg resolve -m b

hg resolve with one unresolved, one resolved:

  $ hg resolve --color=always -l
  [0;31;1mU a[0m
  [0;32;1mR b[0m
