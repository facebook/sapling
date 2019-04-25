  $ newrepo
  $ drawdag <<'EOS'
  > C   # C/a/3=3
  > | D # C/a/2=2
  > |/  # D/a/4=4
  > B
  > |
  > A   # A/a/1=1
  > EOS

  $ hg update -q $C

Log a directory:

  $ hg log -T '{desc}\n' -f a
  C
  A

From non-repo root:

  $ cd a
  $ hg log -G -T '{desc}\n' -f .
  @  C
  :
  o  A
  

Using the follow revset, which is related to repo root:

  $ hg log -G -T '{desc}\n' -r 'follow("a")'
  @  C
  :
  o  A
  
  $ hg log -G -T '{desc}\n' -r 'follow(".")'
  @  C
  |
  o  B
  |
  o  A
  
  $ hg log -G -T '{desc}\n' -r 'follow("relpath:.")'
  @  C
  :
  o  A
  
