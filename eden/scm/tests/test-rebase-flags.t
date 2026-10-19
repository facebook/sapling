
  $ eagerepo
  $ configure mutation-norecord
#require execbit no-eden

  $ tellmeabout() {
  > if [ -x $1 ]; then
  >     echo $1 is an executable file with content:
  >     cat $1
  > else
  >     echo $1 is a plain file with content:
  >     cat $1
  > fi
  > }

  $ cat >> $HGRCPATH << EOF
  > [extensions]
  > rebase=
  > EOF

Test rebasing a single commit that changes flags:
  $ sl init repo
  $ cd repo
  $ echo "A" > foo
  $ sl add foo
  $ chmod +x foo
  $ tellmeabout foo
  foo is an executable file with content:
  A
  $ sl com -m "base"
  $ sl mv foo foo_newloc
  $ sl com -m "move"
  $ sl up .~1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo "B" > foo
  $ sl com -m "change"
  $ sl log -G
  @  commit:      a7f7eece6b0c
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     change
  │
  │ o  commit:      5f41048406b0
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     move
  │
  o  commit:      c0233516197f
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     base
  

  $ sl rebase -r 'desc(move)' -d .
  rebasing 5f41048406b0 "move"
  merging foo and foo_newloc to foo_newloc
  $ sl up tip
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ tellmeabout foo_newloc
  foo_newloc is an executable file with content:
  B

Native rebase combines content and executable-bit changes from either side.
It also preserves text merges, removals, and source copy metadata.

  $ newclientrepo
  $ setconfig nativecheckout.rebaseonenative=true
  $ drawdag <<'EOS'
  > B C
  > |/
  > A
  >   # drawdag.defaultfiles=false
  >   # A/src-set=base\n
  >   # A/dest-clear=base\n (executable)
  >   # A/merged=one\ntwo\nthree\n
  >   # A/removed=removed\n
  >   # B/src-set=source\n
  >   # B/dest-clear=base\n
  >   # B/merged=ONE\ntwo\nthree\n
  >   # B/removed=(removed)
  >   # B/copied=source\n (copied from src-set)
  >   # C/src-set=base\n (executable)
  >   # C/dest-clear=destination\n (executable)
  >   # C/merged=one\ntwo\nTHREE\n
  > EOS
FIXME: Native merge must combine content and mode changes instead of panicking.

  $ sl rebase -q -r $B -d $C > $TESTTMP/rebase-error 2>&1
  [1]
  $ tail -1 $TESTTMP/rebase-error
  SystemError: Rust panic: not implemented
