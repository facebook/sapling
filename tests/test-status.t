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

combining patterns with root and patterns without a root works

  $ hg st a/in_a re:.*b$
  ? a/in_a
  ? b/in_b

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
  never-existed: * (glob)
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

  $ hg status -A -Tjson
  [
   {
    "path": "added",
    "status": "A"
   },
   {
    "copy": "modified",
    "path": "copied",
    "status": "A"
   },
   {
    "path": "removed",
    "status": "R"
   },
   {
    "path": "deleted",
    "status": "!"
   },
   {
    "path": "unknown",
    "status": "?"
   },
   {
    "path": "ignored",
    "status": "I"
   },
   {
    "path": ".hgignore",
    "status": "C"
   },
   {
    "path": "modified",
    "status": "C"
   }
  ]

  $ hg status -A -Tpickle > pickle
  >>> import pickle
  >>> print sorted((x['status'], x['path']) for x in pickle.load(open("pickle")))
  [('!', 'deleted'), ('?', 'pickle'), ('?', 'unknown'), ('A', 'added'), ('A', 'copied'), ('C', '.hgignore'), ('C', 'modified'), ('I', 'ignored'), ('R', 'removed')]
  $ rm pickle

  $ echo "^ignoreddir$" > .hgignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

Test templater support:

  $ hg status -AT "[{status}]\t{if(copy, '{copy} -> ')}{path}\n"
  [M]	.hgignore
  [A]	added
  [A]	modified -> copied
  [R]	removed
  [!]	deleted
  [?]	ignored
  [?]	unknown
  [I]	ignoreddir/file
  [C]	modified
  $ hg status -AT default
  M .hgignore
  A added
  A copied
    modified
  R removed
  ! deleted
  ? ignored
  ? unknown
  I ignoreddir/file
  C modified
  $ hg status -T compact
  abort: "status" not in template map
  [255]

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

Specify working directory revision explicitly, that should be the same as
"hg status"

  $ hg status --change "wdir()"
  M modified
  A added
  A copied
  R removed
  ! deleted
  ? unknown

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

hg status -A --change 1 and revset:

  $ hg status -A --change '1|1'
  M modified
  A added
  A copied
    modified
  R removed
  C deleted

  $ cd ..

hg status with --rev and reverted changes:

  $ hg init reverted-changes-repo
  $ cd reverted-changes-repo
  $ echo a > file
  $ hg add file
  $ hg ci -m a
  $ echo b > file
  $ hg ci -m b

reverted file should appear clean

  $ hg revert -r 0 .
  reverting file
  $ hg status -A --rev 0
  C file

#if execbit
reverted file with changed flag should appear modified

  $ chmod +x file
  $ hg status -A --rev 0
  M file

  $ hg revert -r 0 .
  reverting file

reverted and committed file with changed flag should appear modified

  $ hg co -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ chmod +x file
  $ hg ci -m 'change flag'
  $ hg status -A --rev 1 --rev 2
  M file
  $ hg diff -r 1 -r 2

#endif

hg status of binary file starting with '\1\n', a separator for metadata:

  $ hg init repo5
  $ cd repo5
  >>> open("010a", "wb").write("\1\nfoo")
  $ hg ci -q -A -m 'initial checkin'
  $ hg status -A
  C 010a

  >>> open("010a", "wb").write("\1\nbar")
  $ hg status -A
  M 010a
  $ hg ci -q -m 'modify 010a'
  $ hg status -A --rev 0:1
  M 010a

  $ touch empty
  $ hg ci -q -A -m 'add another file'
  $ hg status -A --rev 1:2 010a
  C 010a

  $ cd ..

test "hg status" with "directory pattern" which matches against files
only known on target revision.

  $ hg init repo6
  $ cd repo6

  $ echo a > a.txt
  $ hg add a.txt
  $ hg commit -m '#0'
  $ mkdir -p 1/2/3/4/5
  $ echo b > 1/2/3/4/5/b.txt
  $ hg add 1/2/3/4/5/b.txt
  $ hg commit -m '#1'

  $ hg update -C 0 > /dev/null
  $ hg status -A
  C a.txt

the directory matching against specified pattern should be removed,
because directory existence prevents 'dirstate.walk()' from showing
warning message about such pattern.

  $ test ! -d 1
  $ hg status -A --rev 1 1/2/3/4/5/b.txt
  R 1/2/3/4/5/b.txt
  $ hg status -A --rev 1 1/2/3/4/5
  R 1/2/3/4/5/b.txt
  $ hg status -A --rev 1 1/2/3
  R 1/2/3/4/5/b.txt
  $ hg status -A --rev 1 1
  R 1/2/3/4/5/b.txt

  $ hg status --config ui.formatdebug=True --rev 1 1
  status = [
      {*'path': '1/2/3/4/5/b.txt'*}, (glob)
  ]

#if windows
  $ hg --config ui.slash=false status -A --rev 1 1
  R 1\2\3\4\5\b.txt
#endif

  $ cd ..

Status after move overwriting a file (issue4458)
=================================================


  $ hg init issue4458
  $ cd issue4458
  $ echo a > a
  $ echo b > b
  $ hg commit -Am base
  adding a
  adding b


with --force

  $ hg mv b --force a
  $ hg st --copies
  M a
    b
  R b
  $ hg revert --all
  reverting a
  undeleting b
  $ rm *.orig

without force

  $ hg rm a
  $ hg st --copies
  R a
  $ hg mv b a
  $ hg st --copies
  M a
    b
  R b

using ui.statuscopies setting
  $ hg st --config ui.statuscopies=true
  M a
    b
  R b
  $ hg st --config ui.statuscopies=false
  M a
  R b

using log status template (issue5155)
  $ hg log -Tstatus -r 'wdir()' -C
  changeset:   2147483647:ffffffffffff
  parent:      0:8c55c58b4c0e
  user:        test
  date:        * (glob)
  files:
  M a
    b
  R b
  

Other "bug" highlight, the revision status does not report the copy information.
This is buggy behavior.

  $ hg commit -m 'blah'
  $ hg st --copies --change .
  M a
  R b

using log status template, the copy information is displayed correctly.
  $ hg log -Tstatus -r. -C
  changeset:   1:6685fde43d21
  tag:         tip
  user:        test
  date:        * (glob)
  summary:     blah
  files:
  M a
    b
  R b
  

  $ cd ..
