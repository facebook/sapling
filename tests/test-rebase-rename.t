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

  $ hg mv b b-renamed
  $ hg ci -m 'rename B'

  $ hg up -q -C 1

  $ hg mv a a-renamed

  $ hg ci -m 'rename A'
  created new head

  $ hg tglog
  @  3: 'rename A'
  |
  | o  2: 'rename B'
  |/
  o  1: 'B'
  |
  o  0: 'A'
  

Rename is tracked:

  $ hg tlog -p --git -r tip
  3: 'rename A' 
  diff --git a/a b/a-renamed
  rename from a
  rename to a-renamed
  
Rebase the revision containing the rename:

  $ hg rebase -s 3 -d 2
  saved backup bundle to $TESTTMP/a/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'rename A'
  |
  o  2: 'rename B'
  |
  o  1: 'B'
  |
  o  0: 'A'
  

Rename is not lost:

  $ hg tlog -p --git -r tip
  3: 'rename A' 
  diff --git a/a b/a-renamed
  rename from a
  rename to a-renamed
  

Rebased revision does not contain information about b (issue3739)

  $ hg log -r 3 --debug
  changeset:   3:3b905b1064f14ace3ad02353b79dd42d32981655
  tag:         tip
  phase:       draft
  parent:      2:920a371a5635af23a26a011ca346cecd1cfcb942
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    3:c4a62b2b64593c8fe0523d4c1ba2e243a8bd4dce
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      a-renamed
  files-:      a
  extra:       branch=default
  extra:       rebase_source=89af05cb38a281f891c6f5581dd027092da29166
  description:
  rename A
  
  

  $ cd ..


  $ hg init b
  $ cd b

  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > b
  $ hg ci -Am B
  adding b

  $ hg cp b b-copied
  $ hg ci -Am 'copy B'

  $ hg up -q -C 1

  $ hg cp a a-copied
  $ hg ci -m 'copy A'
  created new head

  $ hg tglog
  @  3: 'copy A'
  |
  | o  2: 'copy B'
  |/
  o  1: 'B'
  |
  o  0: 'A'
  
Copy is tracked:

  $ hg tlog -p --git -r tip
  3: 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  
Rebase the revision containing the copy:

  $ hg rebase -s 3 -d 2
  saved backup bundle to $TESTTMP/b/.hg/strip-backup/*-backup.hg (glob)

  $ hg tglog
  @  3: 'copy A'
  |
  o  2: 'copy B'
  |
  o  1: 'B'
  |
  o  0: 'A'
  

Copy is not lost:

  $ hg tlog -p --git -r tip
  3: 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  

Rebased revision does not contain information about b (issue3739)

  $ hg log -r 3 --debug
  changeset:   3:98f6e6dbf45ab54079c2237fbd11066a5c41a11d
  tag:         tip
  phase:       draft
  parent:      2:39e588434882ff77d01229d169cdc77f29e8855e
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    3:2232f329d66fffe3930d43479ae624f66322b04d
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      a-copied
  extra:       branch=default
  extra:       rebase_source=0a8162ff18a8900df8df8ef7ac0046955205613e
  description:
  copy A
  
  

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

  $ cd ..
