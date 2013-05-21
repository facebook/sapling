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

  $ mkdir d
  $ echo a > a
  $ hg ci -Am A
  adding a

  $ echo b > d/b
  $ hg ci -Am B
  adding d/b

  $ hg mv d d-renamed
  moving d/b to d-renamed/b (glob)
  $ hg ci -m 'rename B'

  $ hg up -q -C 1

  $ hg mv a a-renamed
  $ echo x > d/x
  $ hg add d/x

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
  diff --git a/d/x b/d/x
  new file mode 100644
  --- /dev/null
  +++ b/d/x
  @@ -0,0 +1,1 @@
  +x
  
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
  diff --git a/d-renamed/x b/d-renamed/x
  new file mode 100644
  --- /dev/null
  +++ b/d-renamed/x
  @@ -0,0 +1,1 @@
  +x
  

Rebased revision does not contain information about b (issue3739)

  $ hg log -r 3 --debug
  changeset:   3:032a9b75e83bff1dcfb6cbfa4ef50a704bf1b569
  tag:         tip
  phase:       draft
  parent:      2:220d0626d185f372d9d8f69d9c73b0811d7725f7
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    3:035d66b27a1b06b2d12b46d41a39adb7a200c370
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      a-renamed d-renamed/x
  files-:      a
  extra:       branch=default
  extra:       rebase_source=73a3ee40125d6f0f347082e5831ceccb3f005f8a
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
