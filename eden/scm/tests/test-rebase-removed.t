#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ configure mutation-norecord
  $ . "$TESTDIR/library.sh"

  $ newserver master
  $ clone master client1
  $ cd client1

  $ drawdag <<'EOS'
  > A1 # A1/A = A42
  > |  # A1/A1 = (removed)
  > |
  > B
  > |
  > A
  > |
  > C
  > EOS

  $ hg rebase -s $B -d $C
  rebasing c84328973e26 "B"
  rebasing 2f1af6263db7 "A1"
  other [source] changed A which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]

  $ hg log -Gr 'all()'
  @  commit:      27652fba03b2
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  │ @  commit:      2f1af6263db7
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     A1
  │ │
  │ x  commit:      c84328973e26
  │ │  user:        test
  │ │  date:        Thu Jan 01 00:00:00 1970 +0000
  │ │  summary:     B
  │ │
  │ o  commit:      9cfaa5b6d3e1
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     A
  │
  o  commit:      96cc3511f894
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     C
  

  $ hg rm -f A

  $ hg resolve -m A
  (no more unresolved files)
  continue: hg rebase --continue

  $ hg rebase --continue
  already rebased c84328973e26 "B" as 27652fba03b2
  rebasing 2f1af6263db7 "A1"

  $ hg log -Gr 'all()'
  o  commit:      8bbb642d1454
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     A1
  │
  o  commit:      27652fba03b2
  │  user:        test
  │  date:        Thu Jan 01 00:00:00 1970 +0000
  │  summary:     B
  │
  │ o  commit:      9cfaa5b6d3e1
  ├─╯  user:        test
  │    date:        Thu Jan 01 00:00:00 1970 +0000
  │    summary:     A
  │
  o  commit:      96cc3511f894
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     C
  
Rebase changes made on copied (forked) source code:

  $ newrepo
  $ drawdag <<'EOS'
  > D E # E/C=1\n2\n3e\n
  > | |
  > B C # C/C=1\n2\n3\n
  > |/  # B/A=1b\n2\n3\n
  > A   # A/A=1\n2\n3\n
  > EOS

 (try normal rebase - fails)
  $ hg rebase -r $E -d $D
  rebasing 8c0ff6bd3515 "E"
  other [source] changed C which local [dest] deleted
  use (c)hanged version, leave (d)eleted, leave (u)nresolved, or input (r)enamed path? u
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ hg rebase --abort
  rebase aborted

 (try rebase with a script saying "C" was renamed to "A")
  $ hg rebase -r $E -d $D --config experimental.rename-cmd='echo A'
  rebasing 8c0ff6bd3515 "E"
  running 'echo A' to find rename destination of C
   trying rename destination: A
  merging A
 (changed in "A" developed in copied "C" are merged back to "A")
  $ hg log -r tip -T '{desc}\n' -p --git
  E
  diff --git a/A b/A
  --- a/A
  +++ b/A
  @@ -1,3 +1,3 @@
   1b
   2
  -3
  +3e
  diff --git a/E b/E
  new file mode 100644
  --- /dev/null
  +++ b/E
  @@ -0,0 +1,1 @@
  +E
  \ No newline at end of file
  

A similar setup. C/C is marked as copied from A.
  $ newrepo
  $ drawdag <<'EOS'
  > D E # E/C=1\n2\n3e\n
  > | |
  > B C # C/C=1\n2\n3\n (copied from A)
  > |/  # B/A=1b\n2\n3\n
  > A   # A/A=1\n2\n3\n
  > EOS

BUG: Changes to the file "C" made in commit "E" shouldn't get lost:
  $ hg rebase -r $E -d $D
  rebasing a0b6e0c8e32c "E"

  $ hg log -r tip -T '{desc}\n' -p --git
  E
  diff --git a/E b/E
  new file mode 100644
  --- /dev/null
  +++ b/E
  @@ -0,0 +1,1 @@
  +E
  \ No newline at end of file
  
