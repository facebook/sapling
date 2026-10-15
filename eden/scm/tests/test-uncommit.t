
#require no-eden


Test uncommit - set up the config

  $ eagerepo
  $ configure mutation-norecord
  $ enable amend

Build up a repo

  $ newrepo
  $ sl bookmark foo

Help for uncommit

  $ sl help uncommit
  sl uncommit [OPTION]... [FILE]...
  
  aliases: unc
  
  uncommit part or all of the current commit
  
      Reverse the effects of an 'sl commit' operation. When run with no
      arguments, hides the current commit and checks out the parent commit, but
      does not revert the state of the working copy. Changes that were contained
      in the uncommitted commit become pending changes in the working copy.
  
      The current commit can be in the middle of a stack. If some of its changes
      are uncommitted, or all of them with "--keep", the commit is rewritten and
      its descendants are restacked onto the new version as 'sl amend' restacks
      them (see "amend.autorestack"). Descendants whose restack would conflict
      are left on the old version; run 'sl restack' to move them. Uncommitting
      every change of a commit that has descendants requires "--keep", which
      leaves the commit in the stack, empty: without it, nothing would replace
      the commit.
  
      By default, you cannot run 'sl uncommit' if there are pending changes in
      the working copy.
  
      You can selectively uncommit files from the current commit by optionally
      specifying a list of files to remove. The specified files are removed from
      the list of changed files in the current commit, but are not modified on
      disk, so they appear as pending changes in the working copy.
  
      Note:
         Running 'sl uncommit' is similar to running 'sl undo --keep'
         immediately after 'sl commit'. However, unlike 'sl undo', which can
         only undo a commit if it was the last operation you performed, 'sl
         uncommit' can uncommit any draft commit you check out.
  
  Options ([+] can be repeated):
  
      --keep                allow an empty commit after uncommiting
   -I --include PATTERN [+] include files matching the given patterns
   -X --exclude PATTERN [+] exclude files matching the given patterns
  
  (some details hidden, use --verbose to show complete help)

Uncommit with no commits should fail

  $ sl uncommit
  abort: cannot uncommit null changeset
  (no changeset checked out)
  [255]

Create some commits

  $ touch files
  $ sl add files
  $ for i in a ab abc abcd abcde; do echo $i > files; echo $i > file-$i; sl add file-$i; sl commit -m "added file-$i"; done
  $ ls
  file-a
  file-ab
  file-abc
  file-abcd
  file-abcde
  files

  $ sl log -G -T '{node} {desc}' --hidden
  @  6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  │
  o  6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  │
  o  abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  │
  o  69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  │
  o  3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  
Simple uncommit off the top, also moves bookmark

  $ sl bookmark
   * foo                       6c4fd43ed714
  $ sl uncommit
  $ sl status
  M files
  A file-abcde
  $ sl bookmark
   * foo                       6db330d65db4

  $ sl log -G -T '{node} {desc}' --hidden
  o  6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  │
  @  6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  │
  o  abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  │
  o  69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  │
  o  3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  

Recommit

  $ sl commit -m 'new change abcde'
  $ sl status
  $ sl heads -T '{node} {desc}'
  0c07a3ccda771b25f1cb1edbd02e683723344ef1 new change abcde (no-eol)

Uncommit of non-existent and unchanged files has no effect
  $ sl uncommit nothinghere
  nothing to uncommit
  [1]
  $ sl status
  $ sl uncommit file-abc
  nothing to uncommit
  [1]
  $ sl status

Uncommit empty commit
  $ echo temp > temp && sl add temp && sl commit -m empty
  $ sl rm temp && sl amend
  $ sl diff -r .^
  $ sl uncommit

Try partial uncommit, also moves bookmark

  $ sl bookmark
   * foo                       0c07a3ccda77
  $ sl uncommit files
  $ sl status
  M files
  $ sl bookmark
   * foo                       3727deee06f7
  $ sl heads -T '{node} {desc}'
  3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcde (no-eol)
  $ sl log -r . -p -T '{node} {desc}'
  3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcdediff -r 6db330d65db4 -r 3727deee06f7 file-abcde
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-abcde	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +abcde
  
  $ sl log -G -T '{node} {desc}' --hidden
  @  3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcde
  │
  │ o  5d2fdaa86d070e06669eb141937268780b14861d empty
  │ │
  │ │ x  aa5000ec01a38397e1e2ad1eddc643c151459a9b empty
  │ ├─╯
  │ x  0c07a3ccda771b25f1cb1edbd02e683723344ef1 new change abcde
  ├─╯
  │ o  6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  ├─╯
  o  6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  │
  o  abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  │
  o  69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  │
  o  3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  
  $ sl commit -m 'update files for abcde'

Uncommit with dirty state

  $ echo "foo" >> files
  $ cat files
  abcde
  foo
  $ sl status
  M files
  $ sl uncommit --config experimental.uncommitondirtywdir=False
  abort: uncommitted changes
  [255]
  $ sl uncommit files
  $ cat files
  abcde
  foo
  $ sl commit -m "files abcde + foo"

Testing with 'experimental.uncommitondirtywdir' on and off

  $ echo "bar" >> files
  $ sl uncommit  --config experimental.uncommitondirtywdir=False
  abort: uncommitted changes
  [255]
  $ sl uncommit
  $ sl commit -m "files abcde + foo"

Uncommit in the middle of a stack

  $ sl checkout '.^^^'
  1 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (leaving bookmark foo)
  $ sl log -r . -p -T '{node} {desc}'
  abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abcdiff -r 69a232e754b0 -r abf2df566fc1 file-abc
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-abc	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +abc
  diff -r 69a232e754b0 -r abf2df566fc1 files
  --- a/files	Thu Jan 01 00:00:00 1970 +0000
  +++ b/files	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -ab
  +abc
  
  $ sl bookmark
     foo                       48e5bd7cd583

Uncommitting every change of a commit with descendants is refused, since
nothing would replace the commit; the hint points at --keep, which leaves it
in the stack, empty. Uncommitting only some of the changes, as below, is
allowed.

  $ sl uncommit
  abort: cannot uncommit every change of a commit that has descendants
  (use --keep to leave the commit in the stack, empty)
  [255]
  $ sl uncommit file-abc files
  abort: cannot uncommit every change of a commit that has descendants
  (use --keep to leave the commit in the stack, empty)
  [255]
  $ sl status
  $ sl bookmark
     foo                       48e5bd7cd583

The rest of this test continues from the commit's parent with its changes
pending, as plain uncommit used to leave it.

  $ sl goto -q '.^'
  $ sl revert -q -r abf2df566fc1 --all
  $ sl status
  M files
  A file-abc
  $ sl heads -T '{node} {desc}'
  48e5bd7cd583eb24164ef8b89185819c84c96ed7 files abcde + foo (no-eol)
  $ sl bookmark
     foo                       48e5bd7cd583
  $ sl commit -m 'new abc'

Partial uncommit in the middle, does not move bookmark

  $ sl checkout '.^'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ sl log -r . -p -T '{node} {desc}'
  69a232e754b08d568c4899475faf2eb44b857802 added file-abdiff -r 3004d2d9b508 -r 69a232e754b0 file-ab
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/file-ab	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +ab
  diff -r 3004d2d9b508 -r 69a232e754b0 files
  --- a/files	Thu Jan 01 00:00:00 1970 +0000
  +++ b/files	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,1 @@
  -a
  +ab
  
  $ sl bookmark
     foo                       48e5bd7cd583
  $ sl uncommit file-ab
  hint[amend-restack]: descendants of 69a232e754b0 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl status
  A file-ab

  $ sl heads -T '{node} {desc}\n'
  8eb87968f2edb7f27f27fe676316e179de65fff6 added file-ab
  5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc
  48e5bd7cd583eb24164ef8b89185819c84c96ed7 files abcde + foo

  $ sl bookmark
     foo                       48e5bd7cd583
  $ sl commit -m 'update ab'
  $ sl status
  $ sl heads -T '{node} {desc}\n'
  f21039c59242b085491bb58f591afc4ed1c04c09 update ab
  5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc
  48e5bd7cd583eb24164ef8b89185819c84c96ed7 files abcde + foo

  $ sl log -G -T '{node} {desc}' --hidden
  @  f21039c59242b085491bb58f591afc4ed1c04c09 update ab
  │
  o  8eb87968f2edb7f27f27fe676316e179de65fff6 added file-ab
  │
  │ o  5dc89ca4486f8a88716c5797fa9f498d13d7c2e1 new abc
  │ │
  │ │ o  48e5bd7cd583eb24164ef8b89185819c84c96ed7 files abcde + foo
  │ │ │
  │ │ │ o  83815831694b1271e9f207cb1b79b2b19275edcb files abcde + foo
  │ │ ├─╯
  │ │ │ o  0977fa602c2fd7d8427ed4e7ee15ea13b84c9173 update files for abcde
  │ │ ├─╯
  │ │ o  3727deee06f72f5ffa8db792ee299cf39e3e190b new change abcde
  │ │ │
  │ │ │ o  5d2fdaa86d070e06669eb141937268780b14861d empty
  │ │ │ │
  │ │ │ │ x  aa5000ec01a38397e1e2ad1eddc643c151459a9b empty
  │ │ │ ├─╯
  │ │ │ x  0c07a3ccda771b25f1cb1edbd02e683723344ef1 new change abcde
  │ │ ├─╯
  │ │ │ o  6c4fd43ed714e7fcd8adbaa7b16c953c2e985b60 added file-abcde
  │ │ ├─╯
  │ │ o  6db330d65db434145c0b59d291853e9a84719b24 added file-abcd
  │ │ │
  │ │ o  abf2df566fc193b3ac34d946e63c1583e4d4732b added file-abc
  │ ├─╯
  │ x  69a232e754b08d568c4899475faf2eb44b857802 added file-ab
  ├─╯
  o  3004d2d9b50883c1538fc754a3aeb55f1b4084f6 added file-a
  
Uncommit with draft parent

  $ sl uncommit
  $ sl phase -r .
  8eb87968f2edb7f27f27fe676316e179de65fff6: draft
  $ sl commit -m 'update ab again'

Uncommit with public parent

  $ sl debugmakepublic "::.^"
  $ sl uncommit
  $ sl phase -r .
  8eb87968f2edb7f27f27fe676316e179de65fff6: public

Partial uncommit with public parent

  $ echo xyz > xyz
  $ sl add xyz
  $ sl commit -m "update ab and add xyz"
  $ sl uncommit xyz
  $ sl status
  A xyz
  $ sl phase -r .
  eba3a9aaec002872b3f74ec1f71cbecc0ad86ac8: draft
  $ sl phase -r ".^"
  8eb87968f2edb7f27f27fe676316e179de65fff6: public

Uncommit leaving an empty changeset

  $ newrepo
  $ sl debugdrawdag <<'EOS'
  > Q
  > |
  > P
  > EOS
  $ sl up Q -q
  $ sl uncommit --keep
  $ sl log -G -T '{desc} FILES: {files}'
  @  Q FILES:
  │
  o  P FILES: P
  
  $ sl status
  A Q

Testing uncommit while merge

  $ newrepo

Create some history

  $ touch a
  $ sl add a
  $ for i in 1 2 3; do echo $i > a; sl commit -m "a $i"; done
  $ sl checkout ea4e33293d4d274a2ba73150733c2612231f398c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ touch b
  $ sl add b
  $ for i in 1 2 3; do echo $i > b; sl commit -m "b $i"; done
  $ sl log -G -T '{node} {desc}' --hidden
  @  2cd56cdde163ded2fbb16ba2f918c96046ab0bf2 b 3
  │
  o  c3a0d5bb3b15834ffd2ef9ef603e93ec65cf2037 b 2
  │
  o  49bb009ca26078726b8870f1edb29fae8f7618f5 b 1
  │
  │ o  990982b7384266e691f1bc08ca36177adcd1c8a9 a 3
  │ │
  │ o  24d38e3cf160c7b6f5ffe82179332229886a6d34 a 2
  ├─╯
  o  ea4e33293d4d274a2ba73150733c2612231f398c a 1
  

Add and expect uncommit to fail on both merge working dir and merge changeset

  $ sl merge 'max(desc(a))'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ sl uncommit --config experimental.uncommitondirtywdir=False
  abort: outstanding uncommitted merge
  [255]

  $ sl uncommit
  abort: cannot uncommit while merging
  [255]

  $ sl status
  M a
  $ sl commit -m 'merge a and b'

  $ sl uncommit
  abort: cannot uncommit merge changeset
  [255]

  $ sl status
  $ sl log -G -T '{node} {desc}' --hidden
  @    c03b9c37bc67bf504d4912061cfb527b47a63c6e merge a and b
  ├─╮
  │ o  2cd56cdde163ded2fbb16ba2f918c96046ab0bf2 b 3
  │ │
  │ o  c3a0d5bb3b15834ffd2ef9ef603e93ec65cf2037 b 2
  │ │
  │ o  49bb009ca26078726b8870f1edb29fae8f7618f5 b 1
  │ │
  o │  990982b7384266e691f1bc08ca36177adcd1c8a9 a 3
  │ │
  o │  24d38e3cf160c7b6f5ffe82179332229886a6d34 a 2
  ├─╯
  o  ea4e33293d4d274a2ba73150733c2612231f398c a 1
Recover added / deleted files

  $ newrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ sl up -q $B
  $ sl rm B
  $ touch C
  $ sl add C
  $ sl commit -m C -q
  $ sl uncommit
  $ sl status
  A C
  R B
  $ ls * | sort
  A
  C

Don't mess up with copies when "dest" of copy was added in the commit we are undoing,
and we have a pending removal of the copied file.
  $ newrepo
  $ touch foo
  $ sl commit -Aqm foo
  $ sl cp foo bar
  $ sl commit -Aqm bar
  $ sl rm bar
  $ sl uncommit
  $ sl st
  $ find .
  ./foo

Copies and renames made by the commit stay recorded as pending copies, whether
the whole commit or only those files are uncommitted
  $ newrepo
  $ echo m > moveme
  $ echo c > c
  $ sl commit -Aqm base
  $ sl mv -q moveme moved
  $ sl cp -q c c2
  $ sl commit -qm work
  $ sl uncommit
  $ sl status -C
  A c2
    c
  A moved
    moveme
  R moveme
  $ sl commit -qm work
  $ sl uncommit moved moveme c2
  $ sl status -C
  A c2
    c
  A moved
    moveme
  R moveme

Partial uncommit replaces the commit: check the mutation it records, that the
bookmark moves, and that undo restores the original commit
  $ newrepo
  $ enable undo
  $ echo a > a
  $ echo b > b
  $ sl commit -Aqm 'add a and b'
  $ sl bookmark book
  $ OLD=$(sl log -r . -T '{node}')
  $ sl bookmark
   * book                      214d02328b23
  $ sl uncommit b
  $ sl status
  A b
  $ sl bookmark
   * book                      0fe586ac1b64
  $ sl log -r 'predecessors(.)' -T '{desc} {files}\n'
  add a and b a b
  add a and b a
  $ sl log -r "successors($OLD)" -T '{desc} {files}\n'
  add a and b a
  $ sl log --hidden -r $OLD -T '{desc}: obsolete={obsolete} op={mutations % "{operation}"}\n'
  add a and b: obsolete=obsolete op=uncommit
  $ sl revert -q --all
  $ sl undo -q
  $ sl log -r . -T '{node|short} {files} obsolete={obsolete}\n'
  214d02328b23 a b obsolete=
  $ sl status
  $ sl bookmark
   * book                      214d02328b23

Uncommit in the middle of a stack rewrites the commit and restacks its
descendants as amend does. With the default amend.autorestack, which restacks
only unchanged commits, they are left behind with a hint.

  $ mkstack() {
  >   newrepo "$1"
  >   echo a > a
  >   sl commit -qAm A
  >   echo b > b
  >   echo x > x
  >   sl commit -qAm B
  >   echo c > c
  >   sl commit -qAm C
  >   sl goto -q 'desc(B)'
  > }

  $ mkstack stack1
  $ sl uncommit x
  hint[amend-restack]: descendants of dc3103bc3938 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ sl log -G -T '{desc} {files}\n'
  @  B b
  │
  │ o  C c
  │ │
  │ x  B b x
  ├─╯
  o  A a
  $ sl status
  A x

With amend.autorestack=no-conflict, the descendants are restacked onto the new
version of the commit.

  $ cat >> $HGRCPATH << 'EOF'
  > [amend]
  > autorestack = no-conflict
  > [rebase]
  > experimental.inmemory = true
  > EOF
  $ mkstack stack2
  $ sl uncommit x
  restacking children automatically (unless they conflict)
  rebasing f7ff3acc4cda "C"
  $ sl log -G -T '{desc} {files}\n'
  o  C c
  │
  @  B b
  │
  o  A a
  $ sl status
  A x

Uncommitting every change of a commit with descendants needs --keep. With it,
the commit stays, empty, as a new version of the old one, and the descendants
are restacked onto it.

  $ mkstack stack3
  $ sl uncommit
  abort: cannot uncommit every change of a commit that has descendants
  (use --keep to leave the commit in the stack, empty)
  [255]
  $ sl uncommit --keep
  restacking children automatically (unless they conflict)
  rebasing f7ff3acc4cda "C"
  $ sl log -G -T '{desc} {files}\n'
  o  C c
  │
  @  B
  │
  o  A a
  $ sl log -r 'predecessors(.) - .' --hidden -T '{desc} {files}\n'
  B b x
  $ sl status
  A b
  A x

A descendant whose restack would conflict stays on the old version with its
own descendants, even with amend.autorestack=always: the uncommitted changes
are in the working copy, so there is no merge to resolve.

  $ mkstack stack4
  $ sl goto -q 'desc(C)'
  $ echo x2 >> x
  $ sl commit -qm D
  $ sl goto -q 'desc(B)'
  $ sl uncommit x --config amend.autorestack=always
  restacking children automatically (unless they conflict)
  rebasing f7ff3acc4cda "C"
  rebasing 93b9cfab1a4d "D"
  restacking would create conflicts (hit merge conflicts in x), so you must run it manually
  (run `sl restack` manually to restack this commit's children)
  $ sl log -G -T '{desc} {files}\n'
  o  C c
  │
  @  B b
  │
  │ o  D x
  │ │
  │ x  C c
  │ │
  │ x  B b x
  ├─╯
  o  A a
  $ sl status
  A x
