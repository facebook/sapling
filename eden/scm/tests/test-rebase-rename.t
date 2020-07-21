#chg-compatible

  $ enable rebase
  $ readconfig <<EOF
  > [alias]
  > tlog  = log --template "{rev}: {node|short} '{desc}' {branches}\n"
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
  moving d/b to d-renamed/b
  $ hg ci -m 'rename B'

  $ hg up -q -C 3ab5da9a5c01faa02c20f2ec4870a4f689c92da6

  $ hg mv a a-renamed
  $ echo x > d/x
  $ hg add d/x

  $ hg ci -m 'rename A'

  $ tglog
  @  3: 73a3ee40125d 'rename A'
  |
  | o  2: 220d0626d185 'rename B'
  |/
  o  1: 3ab5da9a5c01 'B'
  |
  o  0: 1994f17a630e 'A'
  

Rename is tracked:

  $ hg tlog -p --git -r tip
  3: 73a3ee40125d 'rename A' 
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

  $ hg rebase -s 'max(desc(rename))' -d 220d0626d185f372d9d8f69d9c73b0811d7725f7
  rebasing 73a3ee40125d "rename A"

  $ tglog
  @  4: 032a9b75e83b 'rename A'
  |
  o  2: 220d0626d185 'rename B'
  |
  o  1: 3ab5da9a5c01 'B'
  |
  o  0: 1994f17a630e 'A'
  

Rename is not lost:

  $ hg tlog -p --git -r tip
  4: 032a9b75e83b 'rename A' 
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

  $ hg log -r 'max(desc(rename))' --debug
  commit:      032a9b75e83bff1dcfb6cbfa4ef50a704bf1b569
  phase:       draft
  parent:      220d0626d185f372d9d8f69d9c73b0811d7725f7
  parent:      0000000000000000000000000000000000000000
  manifest:    035d66b27a1b06b2d12b46d41a39adb7a200c370
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

  $ hg up -q -C 6c81ed0049f86eccdfa07f4d71b328a6c970b13f

  $ hg cp a a-copied
  $ hg ci -m 'copy A'

  $ tglog
  @  3: 0a8162ff18a8 'copy A'
  |
  | o  2: 39e588434882 'copy B'
  |/
  o  1: 6c81ed0049f8 'B'
  |
  o  0: 1994f17a630e 'A'
  
Copy is tracked:

  $ hg tlog -p --git -r tip
  3: 0a8162ff18a8 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  
Rebase the revision containing the copy:

  $ hg rebase -s 'max(desc(copy))' -d 39e588434882ff77d01229d169cdc77f29e8855e
  rebasing 0a8162ff18a8 "copy A"

  $ tglog
  @  4: 98f6e6dbf45a 'copy A'
  |
  o  2: 39e588434882 'copy B'
  |
  o  1: 6c81ed0049f8 'B'
  |
  o  0: 1994f17a630e 'A'
  

Copy is not lost:

  $ hg tlog -p --git -r tip
  4: 98f6e6dbf45a 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  

Rebased revision does not contain information about b (issue3739)

  $ hg log -r 'max(desc(copy))' --debug
  commit:      98f6e6dbf45ab54079c2237fbd11066a5c41a11d
  phase:       draft
  parent:      39e588434882ff77d01229d169cdc77f29e8855e
  parent:      0000000000000000000000000000000000000000
  manifest:    2232f329d66fffe3930d43479ae624f66322b04d
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

  $ hg update -r 'desc(Unrelated)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo Another unrelated change >> unrelated.txt
  $ hg ci -m "Another unrelated change"

  $ tglog
  @  4: b918d683b091 'Another unrelated change'
  |
  | o  3: 1ac17e43d8aa 'Rename file2 back to file1'
  |/
  o  2: 480101d66d8d 'Unrelated change'
  |
  o  1: be44c61debd2 'Rename file1 to file2'
  |
  o  0: 8ce9a346991d 'Adding file1'
  

  $ hg rebase -s 'desc(Another)' -d 'max(desc(Rename))'
  rebasing b918d683b091 "Another unrelated change"

  $ hg diff --stat -c .
   unrelated.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ cd ..

Verify that copies get preserved (issue4192).
  $ hg init copy-gets-preserved
  $ cd copy-gets-preserved

  $ echo a > a
  $ hg add a
  $ hg commit --message "File a created"
  $ hg copy a b
  $ echo b > b
  $ hg commit --message "File b created as copy of a and modified"
  $ hg copy b c
  $ echo c > c
  $ hg commit --message "File c created as copy of b and modified"
  $ hg copy c d
  $ echo d > d
  $ hg commit --message "File d created as copy of c and modified"

Note that there are four entries in the log for d
  $ tglog --follow d
  @  3: 421b7e82bb85 'File d created as copy of c and modified'
  |
  o  2: 327f772bc074 'File c created as copy of b and modified'
  |
  o  1: 79d255d24ad2 'File b created as copy of a and modified'
  |
  o  0: b220cd6d2326 'File a created'
  
Update back to before we performed copies, and inject an unrelated change.
  $ hg update b220cd6d23263f59a707389fdbe7cef2ce3947ad
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved

  $ echo unrelated > unrelated
  $ hg add unrelated
  $ hg commit --message "Unrelated file created"
  $ hg update 'desc(Unrelated)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Rebase the copies on top of the unrelated change.
  $ hg rebase --source 79d255d24ad2cabeb3e52091338517bb09339f2f --dest 'desc(Unrelated)'
  rebasing 79d255d24ad2 "File b created as copy of a and modified"
  rebasing 327f772bc074 "File c created as copy of b and modified"
  rebasing 421b7e82bb85 "File d created as copy of c and modified"
  $ hg update 'max(desc(File))'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

There should still be four entries in the log for d
  $ tglog --follow d
  @  7: dbb9ba033561 'File d created as copy of c and modified'
  |
  o  6: af74b229bc02 'File c created as copy of b and modified'
  |
  o  5: 68bf06433839 'File b created as copy of a and modified'
  :
  o  0: b220cd6d2326 'File a created'
  
Same steps as above, but with --collapse on rebase to make sure the
copy records collapse correctly.
  $ hg co 'desc(Unrelated)'
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo more >> unrelated
  $ hg ci -m 'unrelated commit is unrelated'
  $ hg rebase -s 68bf06433839d77af722d7baaeecff7c8883c506 --dest 'max(desc(unrelated))' --collapse
  rebasing 68bf06433839 "File b created as copy of a and modified"
  rebasing af74b229bc02 "File c created as copy of b and modified"
  merging b and c to c
  rebasing dbb9ba033561 "File d created as copy of c and modified"
  merging c and d to d
  $ hg co tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

This should show both revision 3 and 0 since 'd' was transitively a
copy of 'a'.

  $ tglog --follow d
  @  9: 5a46b94210e5 'Collapsed revision
  :  * File b created as copy of a and modified
  :  * File c created as copy of b and modified
  :  * File d created as copy of c and modified'
  o  0: b220cd6d2326 'File a created'
  

  $ cd ..
