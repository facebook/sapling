  $ hg init repo1
  $ cd repo1
  $ mkdir a b a/1 b/1 b/2
  $ touch in_root a/in_a b/in_b a/1/in_a_1 b/1/in_b_1 b/2/in_b_2

hg status in repo root:

  $ hg status
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

hg status . in repo root:

  $ hg status .
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

  $ hg status --cwd a
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
  $ hg status --cwd a .
  ? 1/in_a_1
  ? in_a
  $ hg status --cwd a ..
  ? 1/in_a_1
  ? in_a
  ? ../b/1/in_b_1
  ? ../b/2/in_b_2
  ? ../b/in_b
  ? ../in_root

  $ hg status --cwd b
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
  $ hg status --cwd b .
  ? 1/in_b_1
  ? 2/in_b_2
  ? in_b
  $ hg status --cwd b ..
  ? ../a/1/in_a_1
  ? ../a/in_a
  ? 1/in_b_1
  ? 2/in_b_2
  ? in_b
  ? ../in_root

  $ hg status --cwd a/1
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
  $ hg status --cwd a/1 .
  ? in_a_1
  $ hg status --cwd a/1 ..
  ? in_a_1
  ? ../in_a

  $ hg status --cwd b/1
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
  $ hg status --cwd b/1 .
  ? in_b_1
  $ hg status --cwd b/1 ..
  ? in_b_1
  ? ../2/in_b_2
  ? ../in_b

  $ hg status --cwd b/2
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root
  $ hg status --cwd b/2 .
  ? in_b_2
  $ hg status --cwd b/2 ..
  ? ../1/in_b_1
  ? in_b_2
  ? ../in_b
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

  $ hg status
  A added
  R removed
  ! deleted
  ? unknown

hg status modified added removed deleted unknown never-existed ignored:

  $ hg status modified added removed deleted unknown never-existed ignored
  never-existed: No such file or directory
  A added
  R removed
  ! deleted
  ? unknown

  $ hg copy modified copied

hg status -C:

  $ hg status -C
  A added
  A copied
    modified
  R removed
  ! deleted
  ? unknown

hg status -A:

  $ hg status -A
  A added
  A copied
    modified
  R removed
  ! deleted
  ? unknown
  I ignored
  C .hgignore
  C modified


  $ echo "^ignoreddir$" > .hgignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

hg status ignoreddir/file:

  $ hg status ignoreddir/file

hg status -i ignoreddir/file:

  $ hg status -i ignoreddir/file
  I ignoreddir/file
  $ cd ..

Check 'status -q' and some combinations

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

Assert flag1 flag2 [0-same | 1-different]

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

  $ hg init repo4
  $ cd repo4
  $ touch modified removed deleted
  $ hg ci -q -A -m 'initial checkin'
  $ touch added unknown
  $ hg add added
  $ hg remove removed
  $ rm deleted
  $ echo x > modified
  $ hg copy modified copied
  $ hg ci -m 'test checkin' -d "1000001 0"
  $ rm *
  $ touch unrelated
  $ hg ci -q -A -m 'unrelated checkin' -d "1000002 0"

hg status --change 1:

  $ hg status --change 1
  M modified
  A added
  A copied
  R removed

hg status --change 1 unrelated:

  $ hg status --change 1 unrelated

hg status -C --change 1 added modified copied removed deleted:

  $ hg status -C --change 1 added modified copied removed deleted
  M modified
  A added
  A copied
    modified
  R removed

hg status -A --change 1:

  $ hg status -A --change 1
  M modified
  A added
  A copied
    modified
  R removed
  C deleted
