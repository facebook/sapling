
#require diff

  $ newclientrepo repo1
  $ mkdir a b a/1 b/1 b/2
  $ touch in_root a/in_a b/in_b a/1/in_a_1 b/1/in_b_1 b/2/in_b_2

sl status in repo root:

  $ sl status
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

sl status . in repo root:

  $ sl status .
  ? a/1/in_a_1
  ? a/in_a
  ? b/1/in_b_1
  ? b/2/in_b_2
  ? b/in_b
  ? in_root

  $ sl status --cwd a
  ? 1/in_a_1
  ? in_a
  ? ../b/1/in_b_1
  ? ../b/2/in_b_2
  ? ../b/in_b
  ? ../in_root
  $ sl status --cwd a .
  ? 1/in_a_1
  ? in_a
  $ sl status --cwd a ..
  ? 1/in_a_1
  ? in_a
  ? ../b/1/in_b_1
  ? ../b/2/in_b_2
  ? ../b/in_b
  ? ../in_root

  $ sl status --cwd b
  ? ../a/1/in_a_1
  ? ../a/in_a
  ? 1/in_b_1
  ? 2/in_b_2
  ? in_b
  ? ../in_root
  $ sl status --cwd b .
  ? 1/in_b_1
  ? 2/in_b_2
  ? in_b
  $ sl status --cwd b ..
  ? ../a/1/in_a_1
  ? ../a/in_a
  ? 1/in_b_1
  ? 2/in_b_2
  ? in_b
  ? ../in_root

  $ sl status --cwd a/1
  ? in_a_1
  ? ../in_a
  ? ../../b/1/in_b_1
  ? ../../b/2/in_b_2
  ? ../../b/in_b
  ? ../../in_root
  $ sl status --cwd a/1 .
  ? in_a_1
  $ sl status --cwd a/1 ..
  ? in_a_1
  ? ../in_a

  $ sl status --cwd b/1
  ? ../../a/1/in_a_1
  ? ../../a/in_a
  ? in_b_1
  ? ../2/in_b_2
  ? ../in_b
  ? ../../in_root
  $ sl status --cwd b/1 .
  ? in_b_1
  $ sl status --cwd b/1 ..
  ? in_b_1
  ? ../2/in_b_2
  ? ../in_b

  $ sl status --cwd b/2
  ? ../../a/1/in_a_1
  ? ../../a/in_a
  ? ../1/in_b_1
  ? in_b_2
  ? ../in_b
  ? ../../in_root
  $ sl status --cwd b/2 .
  ? in_b_2
  $ sl status --cwd b/2 ..
  ? ../1/in_b_1
  ? in_b_2
  ? ../in_b

combining patterns with root and patterns without a root works

  $ sl st a/in_a re:.*b$
  ? a/in_a
  ? b/in_b

  $ newclientrepo repo2
  $ touch modified removed deleted ignored
  $ echo "ignored" > .gitignore
  $ sl ci -A -m 'initial checkin'
  adding .gitignore
  adding deleted
  adding modified
  adding removed
  $ touch modified added unknown ignored
  $ sl add added
  $ sl remove removed
  $ rm deleted

sl status:

  $ sl status
  A added
  R removed
  ! deleted
  ? unknown

sl status modified added removed deleted unknown never-existed ignored:

  $ sl status modified added removed deleted unknown never-existed ignored
  never-existed: * (glob) (?)
  A added
  R removed
  ! deleted
  ? unknown

  $ sl copy modified copied

sl status -C:

  $ sl status -C
  A added
  A copied
    modified
  R removed
  ! deleted
  ? unknown

sl status -A:

  $ sl status -A
  A added
  A copied
    modified
  R removed
  ! deleted
  ? unknown
  I ignored
  C .gitignore
  C modified

  $ sl status -A -Tjson
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
    "path": ".gitignore",
    "status": "C"
   },
   {
    "path": "modified",
    "status": "C"
   }
  ]

  $ echo "ignoreddir/" > .gitignore
  $ mkdir ignoreddir
  $ touch ignoreddir/file

Test templater support:

  $ sl status -AT "[{status}]\t{if(copy, '{copy} -> ')}{path}\n"
  [M]	.gitignore
  [A]	added
  [A]	modified -> copied
  [R]	removed
  [!]	deleted
  [?]	ignored
  [?]	unknown
  [I]	ignoreddir/file
  [C]	modified
  $ sl status -AT default
  M .gitignore
  A added
  A copied
    modified
  R removed
  ! deleted
  ? ignored
  ? unknown
  I ignoreddir/file
  C modified
  $ sl status -T compact
  abort: "status" not in template map
  [255]
  $ sl status --cwd ignoreddir -AT "{status}: {path} :: {relpath(path)}\n"
  M: ../.gitignore :: ../../.gitignore
  A: ../added :: ../../added
  A: ../copied :: ../../copied
  R: ../removed :: ../../removed
  !: ../deleted :: ../../deleted
  ?: ../ignored :: ../../ignored
  ?: ../unknown :: ../../unknown
  I: file :: ../file
  C: ../modified :: ../../modified

sl status ignoreddir/file:

  $ sl status ignoreddir/file

sl status -i ignoreddir/file:

  $ sl status -i ignoreddir/file
  I ignoreddir/file

Check 'status -q' and some combinations

  $ newclientrepo repo3
  $ touch modified removed deleted ignored
  $ echo "ignored" > .gitignore
  $ sl commit -A -m 'initial checkin'
  adding .gitignore
  adding deleted
  adding modified
  adding removed
  $ touch added unknown ignored
  $ sl add added
  $ echo "test" >> modified
  $ sl remove removed
  $ rm deleted
  $ sl copy modified copied

Ignored but tracked files show up in sl status

  $ sl status ignored
  $ sl add ignored
  the following files are ignored, but still added because they are explicitly specified:
    ignored
  (use 'sl debugignore <file>' to check why they are ignored)
  $ sl status
  M modified
  A added
  A copied
  A ignored
  R removed
  ! deleted
  ? unknown
  $ sl commit -m 'add ignored' ignored
  $ echo >> ignored
  $ sl status
  M ignored
  M modified
  A added
  A copied
  R removed
  ! deleted
  ? unknown
  $ sl rm ignored -f
  $ sl commit -m 'remove ignored' ignored
  $ touch ignored
  $ sl status
  M modified
  A added
  A copied
  R removed
  ! deleted
  ? unknown

Specify working directory revision explicitly, that should be the same as
"sl status"

  $ sl status --change "wdir()"
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
  >     sl status $1 > ../a
  >     sl status $2 > ../b
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

  $ newclientrepo repo4
  $ touch modified removed deleted
  $ sl ci -q -A -m 'initial checkin'
  $ touch added unknown
  $ sl add added
  $ sl remove removed
  $ rm deleted
  $ echo x > modified
  $ sl copy modified copied
  $ sl ci -m 'test checkin' -d "1000001 0"
  $ rm *
  $ touch unrelated
  $ sl ci -q -A -m 'unrelated checkin' -d "1000002 0"

sl status --change 1:

  $ sl status --change 'desc(test)'
  M modified
  A added
  A copied
  R removed

sl status --change 1 unrelated:

  $ sl status --change 'desc(test)' unrelated

sl status -C --change 1 added modified copied removed deleted:

  $ sl status -C --change 'desc(test)' added modified copied removed deleted
  M modified
  A added
  A copied
    modified
  R removed

sl status -A --change 1 and revset:

  $ sl status -A --change 'desc(test)'
  M modified
  A added
  A copied
    modified
  R removed
  C deleted

sl status with --rev and reverted changes:

  $ newclientrepo reverted-changes-repo
  $ echo a > file
  $ sl add file
  $ sl ci -m a
  $ echo b > file
  $ sl ci -m b

reverted file should appear clean

  $ sl revert -r 'desc(a)' .
  reverting file
  $ sl status -A --rev 'desc(a)'
  C file

#if execbit
reverted file with changed flag should appear modified

  $ chmod +x file
  $ sl status -A --rev 'desc(a)'
  M file

  $ sl revert -r 'desc(a)' .
  reverting file

reverted and committed file with changed flag should appear modified

  $ sl co -C .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ chmod +x file
  $ sl ci -m 'change flag'
  $ sl status -A --rev 'desc(b)' --rev 'desc(change)'
  M file
  $ sl diff -r 'desc(b)' -r 'desc(change)'

#endif

sl status of binary file starting with '\1\n', a separator for metadata:

  $ newclientrepo repo5
  >>> _ = open("010a", "wb").write(b"\1\nfoo")
  $ sl ci -q -A -m 'initial checkin'
  $ sl status -A
  C 010a

  >>> _ = open("010a", "wb").write(b"\1\nbar")
  $ sl status -A
  M 010a
  $ sl ci -q -m 'modify 010a'
  $ sl status -A --rev 'desc(initial)':'desc(modify)'
  M 010a

  $ touch empty
  $ sl ci -q -A -m 'add another file'
  $ sl status -A --rev 'desc(modify)':'desc(add)' 010a
  C 010a

#if osx eden

For some reason the repo6 cannot be created when using EdenFS on macOS for some
reason, even though creating even more repos is not an issue on test-rust-checkout.t

#else
test "sl status" with "directory pattern" which matches against files
only known on target revision.

  $ newclientrepo repo6

  $ echo a > a.txt
  $ sl add a.txt
  $ sl commit -m '#0'
  $ mkdir -p 1/2/3/4/5
  $ echo b > 1/2/3/4/5/b.txt
  $ sl add 1/2/3/4/5/b.txt
  $ sl commit -m '#1'

  $ sl goto -C 10edc5093fbb6ac8a9eea22a09d22f54188ab09b > /dev/null
  $ sl status -A
  C a.txt

the directory matching against specified pattern should be removed,
because directory existence prevents 'dirstate.walk()' from showing
warning message about such pattern.

  $ test ! -d 1
  $ sl status -A --rev 'desc("#1")' 1/2/3/4/5/b.txt
  R 1/2/3/4/5/b.txt
  $ sl status -A --rev 'desc("#1")' 1/2/3/4/5
  R 1/2/3/4/5/b.txt
  $ sl status -A --rev 'desc("#1")' 1/2/3
  R 1/2/3/4/5/b.txt
  $ sl status -A --rev 'desc("#1")' 1
  R 1/2/3/4/5/b.txt

  $ sl status --config ui.formatdebug=True --rev 'desc("#1")' 1
  status = [
      {*'path': '1/2/3/4/5/b.txt'*}, (glob)
  ]

#if windows
  $ sl --config ui.slash=false status -A --rev 1 1
  R 1\2\3\4\5\b.txt
  $ HGPLAIN=1 sl --config ui.slash=false status -A --rev 1 1
  R 1/2/3/4/5/b.txt
  $ sl --config ui.slash=true status -A --rev 1 1
  R 1/2/3/4/5/b.txt
#endif

Status after move overwriting a file (issue4458)
=================================================


  $ newclientrepo issue4458
  $ echo a > a
  $ echo b > b
  $ sl commit -Am base
  adding a
  adding b


with --force

  $ sl mv b --force a
  $ sl st --copies
  M a
    b
  R b
  $ sl revert --all
  reverting a
  undeleting b
  $ rm *.orig

without force

  $ sl rm a
  $ sl st --copies
  R a
  $ sl mv b a
  $ sl st --copies
  M a
    b
  R b

using ui.statuscopies setting
  $ sl st --config ui.statuscopies=true
  M a
    b
  R b
  $ sl st --config ui.statuscopies=false
  M a
  R b

using log status template (issue5155)
  $ sl log -Tstatus -r 'wdir()' -C
  commit:      ffffffffffff
  user:        test
  date:        * (glob)
  files:
  M a
    b
  R b
  

Other "bug" highlight, the revision status does not report the copy information.
This is buggy behavior.

  $ sl commit -m 'blah'
  $ sl st --copies --change .
  M a
  R b

using log status template, the copy information is displayed correctly.
  $ sl log -Tstatus -r. -C
  commit:      6685fde43d21
  user:        test
  date:        * (glob)
  summary:     blah
  files:
  M a
    b
  R b
  

  $ cd ..

Make sure we expand env vars in ignore file path.
  $ newclientrepo global-ignore-path
  $ echo ignored > $TESTTMP/global_ignore
  $ touch ignored
  $ sl status --config ui.ignore='$TESTTMP/global_ignore'

  $ cd ..

#if symlink windows
Ignore suspiciously modified symlinks.

  $ newclientrepo suspicious-symlink
  $ ln -s banana foo
  $ sl commit -Aqm foo
  $ rm foo
  $ echo "not\nsymlink" > foo

Force code to think we don't support symlinks to excercise code we want to test.
  $ SL_DEBUG_DISABLE_SYMLINKS=1 sl status --config unsafe.filtersuspectsymlink=true
#endif
#endif
