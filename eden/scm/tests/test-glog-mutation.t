#require no-eden

Test --mutation, -t of `log`.

  $ setconfig diff.git=true

  $ newrepo
  $ drawdag --no-files << 'EOS'
  >   B3  # B3/f=1\n2\n3\n4\n
  >  /    # amend: B1 -> B2 -> B3
  > | B2  # B2/f=1\n2\n3\n
  > |/
  > | B1  # B1/f=1\n2\n
  > |/
  > A     # A/f=1\n
  > EOS

Regular commit graph log:

  $ sl log -Gr: -T '{desc}\n'
  o  B3
  │
  │ x  B2
  ├─╯
  │ x  B1
  ├─╯
  o  A

Mutation graph log:

  $ sl log -Gtr: -T '{desc}\n'
  o  B3
  │
  x  B2
  │
  x  B1
  
  o  A

`-p` and `--stat` are affected by `--mutation` too, for both graph and non-graph log:

  $ sl log -Gr 'predecessors(desc(B3))' -T '{desc}\n' -p --stat --mutation
  o  B3
  │   f |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  │  diff --git a/f b/f
  │  --- a/f
  │  +++ b/f
  │  @@ -1,3 +1,4 @@
  │   1
  │   2
  │   3
  │  +4
  │
  x  B2
  │   f |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  │  diff --git a/f b/f
  │  --- a/f
  │  +++ b/f
  │  @@ -1,2 +1,3 @@
  │   1
  │   2
  │  +3
  │
  x  B1
      f |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
     diff --git a/f b/f
     --- a/f
     +++ b/f
     @@ -1,1 +1,2 @@
      1
     +2

  $ sl log -r 'desc(B3)' -T '{desc}\n' --stat -p --mutation
  B3
   f |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  diff --git a/f b/f
  --- a/f
  +++ b/f
  @@ -1,3 +1,4 @@
   1
   2
   3
  +4

Mutation graph with missing nodes does not crash:
Note: it would be ideal to use dashed lines between B1 and B3, but the blocker is:
graphmod.py dagwalker uses rev numbers, meaning that the nodes must exist in the main
commit dag to have a rev number. If we migrate dagwalker (and its users) to use nodes,
then we can drop ".subdag" from cmdutil._logdagwalker

  $ sl debugstrip 'desc(B2)'
  $ sl log -Gr 'predecessors(desc(B3))' -T '{desc}\n' -p --stat --mutation --traceback
  o  B3
  │   f |  3 +++
  │   1 files changed, 3 insertions(+), 0 deletions(-)
  │
  │  diff --git a/f b/f
  │  --- a/f
  │  +++ b/f
  │  @@ -1,1 +1,4 @@
  │   1
  │  +2
  │  +3
  │  +4
  │
  x  B1
      f |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
     diff --git a/f b/f
     --- a/f
     +++ b/f
     @@ -1,1 +1,2 @@
      1
     +2
