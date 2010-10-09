  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > graphlog=
  > rebase=
  > 
  > [alias]
  > tlog  = log  --template "{rev}: '{desc}' {branches}\n"
  > tglog = tlog --graph
  > EOF


  $ hg init a
  $ cd a

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ hg up -q -C 0

  $ hg mv a a-renamed

  $ hg ci -m 'rename A'
  created new head

  $ hg tglog
  @  2: 'rename A'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  

Rename is tracked:

  $ hg tlog -p --git -r tip
  2: 'rename A' 
  diff --git a/a b/a-renamed
  rename from a
  rename to a-renamed
  
Rebase the revision containing the rename:

  $ hg rebase -s 2 -d 1
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  2: 'rename A'
  |
  o  1: 'B'
  |
  o  0: 'A'
  

Rename is not lost:

  $ hg tlog -p --git -r tip
  2: 'rename A' 
  diff --git a/a b/a-renamed
  rename from a
  rename to a-renamed
  
  $ cd ..


  $ hg init b
  $ cd b

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ hg up -q -C 0

  $ hg cp a a-copied
  $ hg ci -m 'copy A'
  created new head

  $ hg tglog
  @  2: 'copy A'
  |
  | o  1: 'B'
  |/
  o  0: 'A'
  
Copy is tracked:

  $ hg tlog -p --git -r tip
  2: 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  
Rebase the revision containing the copy:

  $ hg rebase -s 2 -d 1
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  2: 'copy A'
  |
  o  1: 'B'
  |
  o  0: 'A'
  
Copy is not lost:

  $ hg tlog -p --git -r tip
  2: 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  
