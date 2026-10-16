
#require no-eden

#testcases tree-store tree-manifest

#if tree-manifest
  $ setconfig experimental.pathhistory-treemanifest=true
#endif

  $ enable rebase
  $ readconfig <<EOF
  > [alias]
  > tlog  = log --template "{node|short} '{desc}' {branches}\n"
  > EOF


  $ sl init a
  $ cd a

  $ mkdir d
  $ echo a > a
  $ sl ci -Am A
  adding a

  $ echo b > d/b
  $ sl ci -Am B
  adding d/b

  $ sl mv d d-renamed
  moving d/b to d-renamed/b
  $ sl ci -m 'rename B'

  $ sl up -q -C .^

  $ sl mv a a-renamed
  $ echo x > d/x
  $ sl add d/x

  $ sl ci -m 'rename A'

  $ tglog
  @  * 'rename A' (glob)
  │
  │ o  * 'rename B' (glob)
  ├─╯
  o  * 'B' (glob)
  │
  o  * 'A' (glob)
  

Rename is tracked:

  $ sl tlog -p --git -r tip
  7685b59c7478 'rename A' 
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

  $ sl rebase -s 'max(desc(rename))' -d 'desc("rename B")'
  rebasing * "rename A" (glob)

  $ tglog
  @  * 'rename A' (glob)
  │
  o  * 'rename B' (glob)
  │
  o  * 'B' (glob)
  │
  o  * 'A' (glob)
  

Rename is not lost:

  $ sl tlog -p --git -r tip
  afa7984ba6cb 'rename A' 
  diff --git a/a b/a-renamed
  rename from a
  rename to a-renamed
  diff --git a/d/x b/d/x
  new file mode 100644
  --- /dev/null
  +++ b/d/x
  @@ -0,0 +1,1 @@
  +x

Rebased revision does not contain information about b (issue3739)

  $ sl log -r 'max(desc(rename))' --debug
  commit:      * (glob)
  phase:       draft
  manifest:    * (glob)
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files+:      a-renamed d/x
  files-:      a
  extra:       branch=default
  extra:       rebase_source=* (glob)
  description:
  rename A

  $ cd ..


  $ sl init b
  $ cd b

  $ echo a > a
  $ sl ci -Am A
  adding a

  $ echo b > b
  $ sl ci -Am B
  adding b

  $ sl cp b b-copied
  $ sl ci -Am 'copy B'

  $ sl up -q -C 6c81ed0049f86eccdfa07f4d71b328a6c970b13f

  $ sl cp a a-copied
  $ sl ci -m 'copy A'

  $ tglog
  @  0a8162ff18a8 'copy A'
  │
  │ o  39e588434882 'copy B'
  ├─╯
  o  6c81ed0049f8 'B'
  │
  o  1994f17a630e 'A'
  
Copy is tracked:

  $ sl tlog -p --git -r tip
  0a8162ff18a8 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  
Rebase the revision containing the copy:

  $ sl rebase -s 'max(desc(copy))' -d 39e588434882ff77d01229d169cdc77f29e8855e
  rebasing 0a8162ff18a8 "copy A"

  $ tglog
  @  98f6e6dbf45a 'copy A'
  │
  o  39e588434882 'copy B'
  │
  o  6c81ed0049f8 'B'
  │
  o  1994f17a630e 'A'
  

Copy is not lost:

  $ sl tlog -p --git -r tip
  98f6e6dbf45a 'copy A' 
  diff --git a/a b/a-copied
  copy from a
  copy to a-copied
  

Rebased revision does not contain information about b (issue3739)

  $ sl log -r 'max(desc(copy))' --debug
  commit:      98f6e6dbf45ab54079c2237fbd11066a5c41a11d
  phase:       draft
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

  $ sl init repo

  $ cd repo

  $ echo testing > file1.txt
  $ sl add file1.txt
  $ sl ci -m "Adding file1"

  $ sl rename file1.txt file2.txt
  $ sl ci -m "Rename file1 to file2"

  $ echo Unrelated change > unrelated.txt
  $ sl add unrelated.txt
  $ sl ci -m "Unrelated change"

  $ sl rename file2.txt file1.txt
  $ sl ci -m "Rename file2 back to file1"

  $ sl goto -r 'desc(Unrelated)'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ echo Another unrelated change >> unrelated.txt
  $ sl ci -m "Another unrelated change"

  $ tglog
  @  b918d683b091 'Another unrelated change'
  │
  │ o  1ac17e43d8aa 'Rename file2 back to file1'
  ├─╯
  o  480101d66d8d 'Unrelated change'
  │
  o  be44c61debd2 'Rename file1 to file2'
  │
  o  8ce9a346991d 'Adding file1'
  

  $ sl rebase -s 'desc(Another)' -d 'max(desc(Rename))'
  rebasing b918d683b091 "Another unrelated change"

  $ sl diff --stat -c .
   unrelated.txt |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)

  $ cd ..

Verify that copies get preserved (issue4192).
  $ sl init copy-gets-preserved
  $ cd copy-gets-preserved

  $ echo a > a
  $ sl add a
  $ sl commit --message "File a created"
  $ sl copy a b
  $ echo b > b
  $ sl commit --message "File b created as copy of a and modified"
  $ sl copy b c
  $ echo c > c
  $ sl commit --message "File c created as copy of b and modified"
  $ sl copy c d
  $ echo d > d
  $ sl commit --message "File d created as copy of c and modified"

Note that there are four entries in the log for d
  $ tglog --follow d
  @  421b7e82bb85 'File d created as copy of c and modified'
  │
  o  327f772bc074 'File c created as copy of b and modified'
  │
  o  79d255d24ad2 'File b created as copy of a and modified'
  │
  o  b220cd6d2326 'File a created'
  
Update back to before we performed copies, and inject an unrelated change.
  $ sl goto b220cd6d23263f59a707389fdbe7cef2ce3947ad
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved

  $ echo unrelated > unrelated
  $ sl add unrelated
  $ sl commit --message "Unrelated file created"
  $ sl goto 'desc(Unrelated)'
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

Rebase the copies on top of the unrelated change.
  $ sl rebase --source 79d255d24ad2cabeb3e52091338517bb09339f2f --dest 'desc(Unrelated)'
  rebasing 79d255d24ad2 "File b created as copy of a and modified"
  rebasing 327f772bc074 "File c created as copy of b and modified"
  rebasing 421b7e82bb85 "File d created as copy of c and modified"
  $ sl goto 'max(desc(File))'
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

There should still be four entries in the log for d
  $ tglog --follow d
  @  dbb9ba033561 'File d created as copy of c and modified'
  │
  o  af74b229bc02 'File c created as copy of b and modified'
  │
  o  68bf06433839 'File b created as copy of a and modified'
  ╷
  o  b220cd6d2326 'File a created'
  
Same steps as above, but with --collapse on rebase to make sure the
copy records collapse correctly.
  $ sl co 'desc(Unrelated)'
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  $ echo more >> unrelated
  $ sl ci -m 'unrelated commit is unrelated'
  $ sl rebase -s 68bf06433839 --dest 'max(desc(unrelated))' --collapse
  rebasing 68bf06433839 "File b created as copy of a and modified"
  rebasing af74b229bc02 "File c created as copy of b and modified"
  rebasing dbb9ba033561 "File d created as copy of c and modified"
  $ sl co tip
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

This should show both revision 3 and 0 since 'd' was transitively a
copy of 'a'.

  $ tglog --follow d
  @  5a46b94210e5 'Collapsed revision
  ╷  * File b created as copy of a and modified
  ╷  * File c created as copy of b and modified
  ╷  * File d created as copy of c and modified'
  o  b220cd6d2326 'File a created'
  

  $ cd ..
