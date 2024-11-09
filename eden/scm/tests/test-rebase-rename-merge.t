  $ enable rebase
  $ setconfig rebase.experimental.inmemory=true
  $ setconfig drawdag.defaultfiles=false
  $ setconfig diff.git=1

Rebase renamed - no content merge:
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/B = A (renamed from A)
  > |/
  > A    # A/A = A
  > EOS
  $ hg rebase -q -r $B -d $C
  $ hg show "successors($B)"
  commit:      140c53e21e48
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A B
  description:
  B
  
  
  diff --git a/A b/B
  rename from A
  rename to B

Rebase renamed - yes content merge:
  $ newclientrepo
  $ drawdag <<EOS
  > C B  # B/B = a\nb\nd\n (renamed from A)
  > |/   # C/A = A\nb\nc\n
  > A    # A/A = a\nb\nc\n
  > EOS
  $ hg rebase -q -r $B -d $C
  $ hg show "successors($B)"
  commit:      e88db2660a78
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A B
  description:
  B
  
  
  diff --git a/A b/B
  rename from A
  rename to B
  --- a/A
  +++ b/B
  @@ -1,3 +1,3 @@
   A
   b
  -c
  +d

Rebase rename already in dest:
  $ newclientrepo
  $ drawdag <<EOS
  > D
  > |
  > C B  # B/B = a\nb\nd\n (renamed from A)
  > |/   # C/B = A\nb\nc\n (renamed from A)
  > A    # A/A = a\nb\nc\n
  > EOS
  $ hg rebase -q -r $B -d $D
  warning: can't find ancestor for 'B' copied from 'A'!
  $ hg show "successors($B)"
  commit:      e2e1c46b067b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       B
  description:
  B
  
  
  diff --git a/B b/B
  --- a/B
  +++ b/B
  @@ -1,3 +1,3 @@
   A
   b
  -c
  +d

Rebase rename with different rename already in dest:
  $ newclientrepo
  $ drawdag <<EOS
  > D
  > |
  > C B  # B/B = a\nb\nd\n (renamed from A)
  > |/   # C/C = A\nb\nc\n (renamed from A)
  > A    # A/A = a\nb\nc\n
  > EOS
  $ hg rebase -q -r $B -d $D
  warning: can't find ancestor for 'B' copied from 'A'!

BUG: doesn't merge rename from both sides
  $ hg show "successors($B)"
  commit:      a03a9e65db8a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       B
  description:
  B
  
  
  diff --git a/B b/B
  new file mode 100644
  --- /dev/null
  +++ b/B
  @@ -0,0 +1,3 @@
  +a
  +b
  +d


Rebase multiple renames with content merge:
  $ newclientrepo
  $ drawdag <<EOS
  >   D  # D/D = a\nb\ne\n (renamed from B)
  >   |
  > C B  # B/B = a\nb\nd\n (renamed from A)
  > |/   # C/A = A\nb\nc\n
  > A    # A/A = a\nb\nc\n
  > EOS
  $ hg rebase -q -s $B -d $C
  $ hg show "successors($B)"
  commit:      e88db2660a78
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       A B
  description:
  B
  
  
  diff --git a/A b/B
  rename from A
  rename to B
  --- a/A
  +++ b/B
  @@ -1,3 +1,3 @@
   A
   b
  -c
  +d
  $ hg show "successors($D)"
  commit:      642994429b16
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       B D
  description:
  D
  
  
  diff --git a/B b/D
  rename from B
  rename to D
  --- a/B
  +++ b/D
  @@ -1,3 +1,3 @@
   A
   b
  -d
  +e
