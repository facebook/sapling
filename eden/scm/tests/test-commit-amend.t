#chg-compatible

  $ enable mutation-norecord
  $ setconfig extensions.treemanifest=!
  $ newrepo
  $ drawdag << 'EOS'
  > B  # B/B=B\n
  > |
  > A  # A/A=A\n
  > EOS

Cannot amend null:

  $ hg ci --amend -m x
  abort: cannot amend null changeset
  (no changeset checked out)
  [255]

Refuse to amend public csets:

  $ hg up -Cq $B
  $ hg phase -r . -p
  $ hg ci --amend
  abort: cannot amend public changesets
  (see 'hg help phases' for details)
  [255]
  $ hg phase -r . -f -d

Nothing to amend:

  $ hg ci --amend -m 'B'
  nothing changed
  [1]

Amending changeset with changes in working dir:
(and check that --message does not trigger an editor)

  $ cat >> $HGRCPATH <<EOF
  > [hooks]
  > pretxncommit.foo = sh -c "echo \\"pretxncommit \$HG_NODE\\"; hg id -r \$HG_NODE"
  > EOF

  $ echo a >> A
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend -m 'amend base1'
  pretxncommit 217e580a9218a74044be7970e41021181317b52b
  217e580a9218

  $ echo '%unset pretxncommit.foo' >> $HGRCPATH

  $ hg diff -c .
  diff -r 4a2df7238c3b -r 217e580a9218 A
  --- a/A	Thu Jan 01 00:00:00 1970 +0000
  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   A
  +a
  diff -r 4a2df7238c3b -r 217e580a9218 B
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +B
  $ hg log -Gr 'all()' -T '{desc}'
  @  amend base1
  |
  o  A
  

Check proper abort for empty message

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > echo "" > "$1"
  > __EOF__

  $ echo a >> A
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  transaction abort! (?)
  rollback completed (?)
  abort: empty commit message
  [255]

Add new file along with modified existing file:

  $ echo C >> C
  $ hg add -q C
  $ hg ci --amend -m 'amend base1 new file'

Remove file that was added in amended commit:
(and test logfile option)
(and test that logfile option do not trigger an editor)

  $ hg rm C
  $ echo 'amend base1 remove new file' > ../logfile
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg ci --amend --logfile ../logfile

  $ hg cat C
  C: no such file in rev 9579b4a5c1df
  [1]

No changes, just a different message:

  $ hg ci --amend -m 'no changes, new message'

  $ hg diff -c .
  diff -r 4a2df7238c3b -r 80f3c49eb411 A
  --- a/A	Thu Jan 01 00:00:00 1970 +0000
  +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,3 @@
   A
  +a
  +a
  diff -r 4a2df7238c3b -r 80f3c49eb411 B
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/B	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +B

Disable default date on commit so when -d isn't given, the old date is preserved:

  $ echo '[defaults]' >> $HGRCPATH
  $ echo 'commit=' >> $HGRCPATH

Test -u/-d:

  $ cat > .hg/checkeditform.sh <<EOF
  > env | grep HGEDITFORM
  > true
  > EOF

  $ HGEDITOR="sh .hg/checkeditform.sh" hg ci --amend -u foo -d '1 0'
  HGEDITFORM=commit.amend.normal

  $ echo a >> A
  $ hg ci --amend -u foo -d '1 0'

  $ hg log -r .
  changeset:   7:815553afc946
  parent:      0:4a2df7238c3b
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     no changes, new message
  

Open editor with old commit message if a message isn't given otherwise:

  $ cat > editor.sh << '__EOF__'
  > #!/bin/sh
  > cat $1
  > echo "another precious commit message" > "$1"
  > __EOF__

at first, test saving last-message.txt

  $ cat > .hg/hgrc << '__EOF__'
  > [hooks]
  > pretxncommit.test-saving-last-message = false
  > __EOF__

  $ rm -f .hg/last-message.txt
  $ hg commit --amend -m "message given from command line"
  transaction abort!
  rollback completed
  abort: pretxncommit.test-saving-last-message hook exited with status 1
  [255]

  $ cat .hg/last-message.txt
  message given from command line (no-eol)

  $ rm -f .hg/last-message.txt

  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: added B
  HG: changed A
  transaction abort!
  rollback completed
  abort: pretxncommit.test-saving-last-message hook exited with status 1
  [255]

  $ cat .hg/last-message.txt
  another precious commit message

  $ cat > .hg/hgrc << '__EOF__'
  > [hooks]
  > pretxncommit.test-saving-last-message =
  > __EOF__

then, test editing custom commit message

  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  no changes, new message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: added B
  HG: changed A

Same, but with changes in working dir (different code path):

  $ echo a >> A
  $ HGEDITOR="\"sh\" \"`pwd`/editor.sh\"" hg commit --amend
  another precious commit message
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: foo
  HG: branch 'default'
  HG: added B
  HG: changed A

  $ rm editor.sh
  $ hg log -r .
  changeset:   9:f7f2c5aae908
  parent:      0:4a2df7238c3b
  user:        foo
  date:        Thu Jan 01 00:00:01 1970 +0000
  summary:     another precious commit message
  

Moving bookmarks, preserve active bookmark:

  $ newrepo
  $ drawdag << 'EOS'
  > a
  > EOS
  $ hg book -r $a book1
  $ hg book -r $a book2
  $ hg up -q book1
  $ hg ci --amend -m 'move bookmarks'
  $ hg book
   transaction abort! (?)
   rollback completed (?)
   * book1                     1:919d9f835a8e
     book2                     1:919d9f835a8e

abort does not loose bookmarks
(note: with fsmonitor, transaction started before checking commit message)


Restore global hgrc

  $ cat >> $HGRCPATH <<EOF
  > [defaults]
  > commit=-d '0 0'
  > EOF

Refuse to amend during a merge:

  $ newrepo
  $ drawdag <<'EOS'
  > Y Z
  > |/
  > X
  > EOS
  $ hg up -q $Y
  $ hg merge -q $Z
  $ hg ci --amend
  abort: cannot amend while merging
  [255]

Refuse to amend if there is a merge conflict (issue5805):

  $ newrepo
  $ drawdag <<'EOS'
  > Y  # Y/X=Y
  > |
  > X
  > EOS
  $ hg up -q $X
  $ echo c >> X
  $ hg up $Y -t :fail -q
  [1]
  $ hg resolve -l
  U X

  $ hg ci --amend
  abort: unresolved merge conflicts (see 'hg help resolve')
  [255]

Follow copies/renames (including issue4405):

  $ newrepo
  $ drawdag <<'EOS'
  > B   # B/B=A (renamed from A)
  > |
  > A
  > EOS

  $ hg up -q $B
  $ echo 1 >> B
  $ hg ci --amend -m 'B-amended'
  $ hg log -r . -T '{file_copies}\n'
  B (A)

  $ hg mv B C
  $ hg ci --amend -m 'C'
  $ hg log -r . -T '{file_copies}\n'
  C (A)

Move added file (issue3410):

  $ newrepo
  $ drawdag <<'EOS'
  > A
  > EOS

  $ hg up -q $A
  $ hg mv A B
  $ hg ci --amend -m 'B'
  $ hg log -r . --template "{file_copies}\n"
  

Obsolete information

  $ hg log -r 'predecessors(.)' --hidden -T '{desc}\n'
  A
  B

Amend a merge. Make it trickier by including renames.

  $ newrepo
  $ drawdag << 'EOS'
  >      # D/D=3
  > D    # C/D=2
  > |\   # B/D=1
  > B C  # B/B=X (renamed from X)
  > | |  # C/C=Y (renamed from Y)
  > X Y
  > EOS

  $ hg up -q $D
  $ hg debugrename B
  B renamed from X:44f0fe2c7b2f8e25d302364ca8d50f37f9bfb143
  $ hg debugrename C
  C renamed from Y:949988db577d2987b8dc29aeb0467aad77fd2005

  $ echo 4 >> D
  $ hg mv B B2
  $ hg mv C C2
  $ hg commit --amend -m D2
  $ hg log -r. -T '{desc}\n'
  D2
  $ hg cat -r. D
  34

  $ hg debugrename B2
  B2 renamed from B:668baf98ee11de8040fa6e9d9b477cb85157750a
  $ hg debugrename C2
  C2 renamed from C:9eeb74a40ee18c256903a5b1d572e0debc1f4cb8

Undo renames

  $ hg mv B2 B
  $ hg mv C2 C
  $ hg commit --amend -m D3
  $ hg debugrename B
  B renamed from X:44f0fe2c7b2f8e25d302364ca8d50f37f9bfb143
  $ hg debugrename C
  C renamed from Y:949988db577d2987b8dc29aeb0467aad77fd2005

Undo merge conflict resolution

  $ hg log -GT '{desc}\n' -f D
  @    D3
  |\
  | o  C
  | |
  | ~
  o  B
  |
  ~

 (This is suboptimal. It should only show B without D4)
  $ printf 1 > D
  $ hg commit --amend -m D4
  $ hg log -GT '{desc}\n' -f D
  @    D4
  |\
  | o  C
  | |
  | ~
  o  B
  |
  ~

  $ printf 2 > D
  $ hg commit --amend -m D4
  $ hg log -GT '{desc}\n' -f D
  o  C
  |
  ~

Amend a merge, with change/deletion conflict.
Sadly, this test shows internals are inconsistent.

  $ newrepo
  $ drawdag << 'EOS'
  >        # E/A=D
  >   E    # E/B=C
  >   |\   # C/A=(removed)
  >   C D  # C/B=C
  >   |/   # D/A=D
  >   |    # D/B=(removed)
  >  /|
  > A B
  > EOS

  $ hg files -r $E
  A
  B

  $ hg up -q $E

  $ hg log -f -T '{desc}' -G A
  o    D
  |\
  | ~
  o  A
  
  $ hg log -f -T '{desc}' -G B
  o    C
  |\
  | ~
  o  B
  
  $ hg log -r. -T '{files}'

  $ hg rm A B
  $ hg ci --amend -m E2
  $ hg log --removed -f -T '{desc}' -G A
  o    D
  |\
  | ~
  | o  C
  |/|
  | ~
  o  A
  
  $ hg log --removed -f -T '{desc}' -G B
  @    E2
  |\
  | o    D
  | |\
  | | ~
  o |  C
  |\|
  ~ |
   /
  o  B
  

 Undo the removal

  $ printf C > B
  $ printf D > A
  $ hg ci --amend -m E3
  $ hg log -fr tip -T '{desc}' -G A
  o    D
  |\
  | ~
  o  A
  
  $ hg log -fr tip -T '{desc}' -G B
  o    C
  |\
  | ~
  o  B
  

  $ hg log -r. -T '{files}'
  B (no-eol)

Test that amend with --edit invokes editor forcibly

  $ newrepo
  $ echo A | hg debugdrawdag
  $ hg up -q A

  $ HGEDITOR=cat hg commit --amend -m "editor should be suppressed"
  $ hg log -r. -T '{desc}\n'
  editor should be suppressed

  $ HGEDITOR=cat hg commit --amend -m "editor should be invoked" --edit
  editor should be invoked
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: bookmark 'A'
  HG: added A

  $ hg log -r. -T '{desc}\n'
  editor should be invoked

Test that "diff()" in committemplate works correctly for amending

  $ newrepo
  $ cat >> .hg/hgrc <<EOF
  > [committemplate]
  > changeset.commit.amend = {desc}\n
  >     HG: M: {file_mods}
  >     HG: A: {file_adds}
  >     HG: R: {file_dels}
  >     {splitlines(diff()) % 'HG: {line}\n'}
  > EOF

  $ echo A | hg debugdrawdag
  $ hg up -q A

  $ HGEDITOR=cat hg commit --amend -e -m "expecting diff of A"
  expecting diff of A
  
  HG: M: 
  HG: A: A
  HG: R: 
  HG: diff -r 000000000000 A
  HG: --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  HG: +++ b/A	Thu Jan 01 00:00:00 1970 +0000
  HG: @@ -0,0 +1,1 @@
  HG: +A
  HG: \ No newline at end of file

#if execbit

Test if amend preserves executable bit changes

  $ newrepo
  $ drawdag <<'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up -q $B
  $ chmod +x A
  $ hg ci -m chmod
  $ hg ci --amend -m "chmod amended"
  $ hg ci --amend -m "chmod amended second time"
  $ hg log -p --git -r .
  changeset:   4:b4aab18bba3e
  parent:      1:112478962961
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     chmod amended second time
  
  diff --git a/A b/A
  old mode 100644
  new mode 100755
  
#endif

Test amend with file inclusion options
--------------------------------------

These tests ensure that we are always amending some files that were part of the
pre-amend commit. We want to test that the remaining files in the pre-amend
commit were not changed in the amended commit. We do so by performing a diff of
the amended commit against its parent commit.

  $ newrepo testfileinclusions
  $ echo a > a
  $ echo b > b
  $ hg commit -Aqm "Adding a and b"

Only add changes to a particular file
  $ echo a >> a
  $ echo b >> b
  $ hg commit --amend -I a
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,1 @@
  +b

  $ echo a >> a
  $ hg commit --amend b
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,2 @@
  +b
  +b

Exclude changes to a particular file
  $ echo b >> b
  $ hg commit --amend -X a
  $ hg diff --git -r null -r .
  diff --git a/a b/a
  new file mode 100644
  --- /dev/null
  +++ b/a
  @@ -0,0 +1,2 @@
  +a
  +a
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,3 @@
  +b
  +b
  +b

Check the addremove flag
  $ echo c > c
  $ rm a
  $ hg commit --amend -A
  removing a
  adding c
  $ hg diff --git -r null -r .
  diff --git a/b b/b
  new file mode 100644
  --- /dev/null
  +++ b/b
  @@ -0,0 +1,3 @@
  +b
  +b
  +b
  diff --git a/c b/c
  new file mode 100644
  --- /dev/null
  +++ b/c
  @@ -0,0 +1,1 @@
  +c
