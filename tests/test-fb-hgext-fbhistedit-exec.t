TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ . "$TESTDIR/histedit-helpers.sh"

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > fbhistedit=
  > histedit=
  > rebase=
  > EOF

  $ initrepo ()
  > {
  >     hg init r
  >     cd r
  >     for x in a b c d e f ; do
  >         echo $x > $x
  >         hg add $x
  >         hg ci -m $x
  >     done
  > }

  $ initrepo

log before edit

  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  

exec & continue should not preserve hashes

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "this should be printed to stdout"
  > exec echo "this should be printed to stderr" >&2
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  this should be printed to stdout
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  this should be printed to stderr

  $ hg log --graph
  @  changeset:   5:652413bf663e
  |  tag:         tip
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     f
  |
  o  changeset:   4:e860deea161a
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     e
  |
  o  changeset:   3:055a42cdd887
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     d
  |
  o  changeset:   2:177f92b77385
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     c
  |
  o  changeset:   1:d2ae7f538514
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     b
  |
  o  changeset:   0:cb9a9f314b8b
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     a
  
ensure we are properly executed in a shell
  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "foo" >/dev/null && exit 0
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

a failing command should drop us into the shell

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
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

retry should work

  $ hg histedit --retry
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1
  [1]

continue should work

  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 2' failed with exit status 2
  [1]

retry after consecutive failed execs

  $ hg histedit --retry
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 2' failed with exit status 2
  [1]

continue after consecutive failed execs

  $ hg histedit --continue
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 3' failed with exit status 3
  [1]

retry after the last entry

  $ hg histedit --retry
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 3' failed with exit status 3
  [1]

continue after the last entry

  $ hg histedit --continue

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
retry should try to execute the command again and continue if succeeded

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

  $ hg histedit --edit-plan --commands - 2>&1 << EOF| fixbundle
  > exec echo "Called"
  > exec exit 2
  > edit 652413bf663e f
  > EOF

  $ hg histedit --retry
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Called
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'exit 2' failed with exit status 2
  [1]

retry should fail when working copy has pending changes

  $ echo "g" >> g
  $ hg add g
  $ hg histedit --retry
  abort: working copy has pending changes
  (amend, commit, or revert them and run histedit --retry, or abort with histedit --abort)
  [255]

  $ hg revert -ar .
  forgetting g

retry should fail when used on non-exec histedit command

  $ hg histedit --continue
  adding f
  Editing (652413bf663e), you may commit or record as needed now.
  (hg histedit --continue to resume)
  [1]

  $ hg histedit --retry
  abort: no exec in progress
  (if you want to continue a non-exec histedit command use "histedit --continue" instead.)
  [255]
  $ hg histedit --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
abort should work

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec exit 1
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  Command 'exit 1' failed with exit status 1

  $ hg histedit --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

Multiple exec commands must work

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > exec exit 0
  > pick e860deea161a e
  > exec exit 0
  > exec exit 0
  > exec exit 0
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ hg log --template '{node|short} {desc}' --graph
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

abort on a failing command, e.g when we have children

  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > pick e860deea161a e
  > exec echo "added" > added && hg add added && hg commit --amend
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  abort: cannot amend changeset with children

  $ hg histedit --abort
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

test 'execr' executing in the current directory

  $ hg up 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ mkdir subdir
  $ cd subdir
  $ hg histedit 177f92b77385 --commands - 2>&1 << EOF| fixbundle
  > pick 177f92b77385 c
  > pick 055a42cdd887 d
  > execr echo "added long" >> subfile
  > pick e860deea161a e
  > execr echo "added short" >> subfile
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls
  subfile
  $ cat subfile
  added long
  added short
  $ cd ..

Test that we can recover exec with amend on

  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > amend=
  > [experimental]
  > evolution=createmarkers, allowunstable
  > EOF

  $ hg up -q tip

Test continue a stopped histedit

  $ hg log -G --hidden -T '{node|short} {desc|firstline}\n'
  @  652413bf663e f
  |
  o  e860deea161a e
  |
  o  055a42cdd887 d
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
  $ hg histedit 055a42cdd887 --commands - 2>&1 << EOF
  > pick e860deea161a e
  > pick 055a42cdd887 d
  > exec false
  > pick 652413bf663e f
  > EOF
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'false' failed with exit status 1
  [1]
  $ echo d >> d
  $ hg log -G --hidden -T '{node|short} {desc|firstline}\n'
  @  883a5225844a d
  |
  o  d8249471110a e
  |
  | o  652413bf663e f
  | |
  | o  e860deea161a e
  | |
  | o  055a42cdd887 d
  |/
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
  $ hg commit --amend -m d
  $ hg log -G --hidden -T '{node|short} {desc|firstline}\n'
  @  8800a5180f91 d
  |
  | x  883a5225844a d
  |/
  o  d8249471110a e
  |
  | o  652413bf663e f
  | |
  | o  e860deea161a e
  | |
  | o  055a42cdd887 d
  |/
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
  $ hg histedit --continue --traceback
  $ hg log -G --hidden -T '{node|short} {desc|firstline}\n'
  @  0d9a4961b100 f
  |
  o  8800a5180f91 d
  |
  | x  883a5225844a d
  |/
  o  d8249471110a e
  |
  | x  652413bf663e f
  | |
  | x  e860deea161a e
  | |
  | x  055a42cdd887 d
  |/
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

Test abort a stopped histedit with obsmarkers

  $ hg histedit d8249471110a --commands - 2>&1 << EOF
  > pick 8800a5180f91 d
  > stop d8249471110a e
  > exec false
  > pick 0d9a4961b100 f
  > EOF
  Changes committed as 1d7ed205640e. You may amend the changeset now.
  When you are done, run hg histedit --continue to resume
  [1]
  $ hg histedit --edit-plan
  $ hg histedit --continue
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Command 'false' failed with exit status 1
  [1]
  $ echo e >> e
  $ hg commit --amend -m e
  $ hg histedit --abort --traceback
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G --hidden -T '{node|short} {desc|firstline}\n'
  x  2b451ea9fdb0 e
  |
  | x  1d7ed205640e e
  |/
  x  abce911bf7cf d
  |
  | @  0d9a4961b100 f
  | |
  | o  8800a5180f91 d
  | |
  | | x  883a5225844a d
  | |/
  | o  d8249471110a e
  |/
  | x  652413bf663e f
  | |
  | x  e860deea161a e
  | |
  | x  055a42cdd887 d
  |/
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
Test amend inside exec rule:

  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  0d9a4961b100 f
  |
  o  8800a5180f91 d
  |
  o  d8249471110a e
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  

  $ hg histedit 8800a5180f91 --commands - 2>&1 << EOF
  > pick 8800a5180f91 d
  > exec hg commit --amend -m "d (amended)"
  > pick 0d9a4961b100 f
  > EOF
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg log -G -T '{node|short} {desc|firstline}\n'
  @  5aeafddb5246 f
  |
  o  6bd17118649c d (amended)
  |
  o  d8249471110a e
  |
  o  177f92b77385 c
  |
  o  d2ae7f538514 b
  |
  o  cb9a9f314b8b a
  
Use exec to create a new commit at the bottom of a stack (that is,
on top of a public commit).  Histedit shouldn't try to obsolete the
public commit.

  $ hg phase -p ::d8249471110a
  $ hg histedit --commands - 2>&1 << EOF
  > exec touch x; hg add x; hg commit -m "x (inserted)"
  > pick 6bd17118649c
  > pick 5aeafddb5246
  > EOF
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg log -G -T '{node|short} [{phase}] {desc|firstline}\n'
  @  325ec50aef26 [draft] f
  |
  o  6fa3f6d34b50 [draft] d (amended)
  |
  o  c90a2aff5a5c [draft] x (inserted)
  |
  o  d8249471110a [public] e
  |
  o  177f92b77385 [public] c
  |
  o  d2ae7f538514 [public] b
  |
  o  cb9a9f314b8b [public] a
  
