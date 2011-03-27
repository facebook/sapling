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
  
  $ cd ..


Test rebase across repeating renames:

  $ hg init repo

  $ cd repo

  $ echo testing > file1.txt
  $ hg add file1.txt
  $ hg ci -m "Adding file1"

  $ hg rename file1.txt file2.txt
  $ hg ci -m "Rename file1 to file2"

  $ echo Unrelated change > unrelated.txt
  $ hg add unrelated.txt
  $ hg ci -m "Unrelated change"

  $ hg rename file2.txt file1.txt
  $ hg ci -m "Rename file2 back to file1"

  $ hg update -r -2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo Another unrelated change >> unrelated.txt
  $ hg ci -m "Another unrelated change"
  created new head

  $ hg tglog
  @  4: 'Another unrelated change'
  |
  | o  3: 'Rename file2 back to file1'
  |/
  o  2: 'Unrelated change'
  |
  o  1: 'Rename file1 to file2'
  |
  o  0: 'Adding file1'
  

  $ hg rebase -s 4 -d 3
  saved backup bundle to $TESTTMP/repo/.hg/strip-backup/*-backup.hg (glob)

  $ hg diff --stat -c .
   unrelated.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

