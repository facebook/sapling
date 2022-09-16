#debugruntest-compatible
  $ enable absorb remotefilelog

Create repo

  $ newrepo
  $ echo remotefilelog >> .hg/requires

  $ drawdag << 'EOS'
  > C
  > |
  > B
  > |
  > A
  > EOS
  $ hg up $C -q

Edit & absorb

  $ echo 1 >> A
  $ echo 2 >> B
  $ hg absorb
  showing changes for A
          @@ -0,1 +0,1 @@
  426bada -A
  426bada +A1
  showing changes for B
          @@ -0,1 +0,1 @@
  1124789 -B
  1124789 +B2
  
  2 changesets affected
  1124789 B
  426bada A
  apply changes (yn)?  y
  2 of 2 chunks applied

Check result

  $ hg log -Gpr 'all()' --config diff.git=1 -T '{desc}\n'
  @  C
  │  diff --git a/C b/C
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/C
  │  @@ -0,0 +1,1 @@
  │  +C
  │  \ No newline at end of file
  │
  o  B
  │  diff --git a/B b/B
  │  new file mode 100644
  │  --- /dev/null
  │  +++ b/B
  │  @@ -0,0 +1,1 @@
  │  +B2
  │
  o  A
     diff --git a/A b/A
     new file mode 100644
     --- /dev/null
     +++ b/A
     @@ -0,0 +1,1 @@
     +A1
  
