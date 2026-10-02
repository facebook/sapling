
#require no-eden


  $ eagerepo
  $ . "$TESTDIR/histedit-helpers.sh"

  $ enable fbhistedit histedit rebase

  $ initrepo ()
  > {
  >     sl init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         sl add $x
  >         sl ci -m $x
  >     done
  > }

  $ initrepo

log before edit

  $ sl log --graph
  @  commit:      652413bf663e
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     f
  │
  o  commit:      e860deea161a
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     e
  │
  o  commit:      055a42cdd887
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     d
  │
  o  commit:      177f92b77385
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     c
  │
  o  commit:      d2ae7f538514
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     b
  │
  o  commit:      cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

show-plan before starting a histedit shows the starting plan without running it

  $ sl histedit --show-plan 177f92b77385
  histedit plan for 177f92b77385 to 652413bf663e (to run it, pass each line to "histedit --plan", or the whole plan file to "histedit --commands"):
      pick 177f92b77385 c
      pick 055a42cdd887 d
      pick e860deea161a e
      pick 652413bf663e f
  $ sl histedit --show-plan -r 'desc(e)'
  histedit plan for e860deea161a to 652413bf663e (to run it, pass each line to "histedit --plan", or the whole plan file to "histedit --commands"):
      pick e860deea161a e
      pick 652413bf663e f
  $ sl log -r . -T '{desc}\n'
  f
  $ sl histedit --show-plan cb9a9f314b8b::
  histedit plan for cb9a9f314b8b to 652413bf663e (to run it, pass each line to "histedit --plan", or the whole plan file to "histedit --commands"):
      pick cb9a9f314b8b a
      pick d2ae7f538514 b
      pick 177f92b77385 c
      pick 055a42cdd887 d
      pick e860deea161a e
      pick 652413bf663e f
  $ sl histedit --show-plan --config histedit.defaultrev='desc(d)'
  histedit plan for 055a42cdd887 to 652413bf663e (to run it, pass each line to "histedit --plan", or the whole plan file to "histedit --commands"):
      pick 055a42cdd887 d
      pick e860deea161a e
      pick 652413bf663e f
  $ sl histedit --show-plan 'desc(c)' 'desc(d)'
  abort: histedit requires exactly one ancestor revision
  [255]

a failing command should drop us into the shell

  $ sl histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > exec exit 2
  > pick 652413bf663e f
  > exec exit 3
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

show-plan shows the remaining plan while a histedit is running

  $ sl histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 1
      exec exit 2
      pick 652413bf663e f
      exec exit 3

arguments are not allowed while a histedit is running

  $ sl histedit --show-plan 177f92b77385
  abort: no arguments allowed with --show-plan
  [255]

continue should work

  $ sl histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 2' failed with exit status 2
  [1]

show-plan after consecutive failed execs

  $ sl histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 2
      pick 652413bf663e f
      exec exit 3

continue after consecutive failed execs

  $ sl histedit --continue
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 3' failed with exit status 3
  [1]

show-plan after the last entry

  $ sl histedit --show-plan
  histedit plan (call "histedit --continue/--retry" to resume it or "histedit --abort" to abort it):
      exec exit 3

continue after the last entry

  $ sl histedit --continue

  $ sl log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  │
  o  e860deea161a e
  │
  o  055a42cdd887 d
  │
  o  177f92b77385 c
  │
  o  d2ae7f538514 b
  │
  o  cb9a9f314b8b a
  
