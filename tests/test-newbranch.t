  $ branchcache=.hg/cache/branchheads

  $ hg init t
  $ cd t

  $ hg branches
  $ echo foo > a
  $ hg add a
  $ hg ci -m "initial"
  $ hg branch foo
  marked working directory as branch foo
  $ hg branch
  foo
  $ hg ci -m "add branch name"
  $ hg branch bar
  marked working directory as branch bar
  $ hg ci -m "change branch name"

Branch shadowing:

  $ hg branch default
  abort: a branch of the same name already exists (use 'hg update' to switch to it)
  [255]

  $ hg branch -f default
  marked working directory as branch default

  $ hg ci -m "clear branch name"
  created new head

There should be only one default branch head

  $ hg heads .
  changeset:   3:9d567d0b51f9
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     clear branch name
  

  $ hg co foo
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch
  foo
  $ echo bleah > a
  $ hg ci -m "modify a branch"

  $ hg merge default
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg branch
  foo
  $ hg ci -m "merge"

  $ hg log
  changeset:   5:dc140083783b
  branch:      foo
  tag:         tip
  parent:      4:98d14f698afe
  parent:      3:9d567d0b51f9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge
  
  changeset:   4:98d14f698afe
  branch:      foo
  parent:      1:0079f24813e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a branch
  
  changeset:   3:9d567d0b51f9
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     clear branch name
  
  changeset:   2:ed2bbf4e0102
  branch:      bar
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     change branch name
  
  changeset:   1:0079f24813e2
  branch:      foo
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add branch name
  
  changeset:   0:db01e8ea3388
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     initial
  
  $ hg branches
  foo                            5:dc140083783b
  default                        3:9d567d0b51f9 (inactive)
  bar                            2:ed2bbf4e0102 (inactive)

  $ hg branches -q
  foo
  default
  bar

Test for invalid branch cache:

  $ hg rollback
  repository tip rolled back to revision 4 (undo commit)
  working directory now based on revisions 4 and 3

  $ cp $branchcache .hg/bc-invalid

  $ hg log -r foo
  changeset:   4:98d14f698afe
  branch:      foo
  tag:         tip
  parent:      1:0079f24813e2
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify a branch
  
  $ cp .hg/bc-invalid $branchcache

  $ hg --debug log -r foo
  invalidating branch cache (tip differs)
  changeset:   4:98d14f698afeaff8cb612dcf215ce95e639effc3
  branch:      foo
  tag:         tip
  parent:      1:0079f24813e2b73a891577c243684c5066347bc8
  parent:      -1:0000000000000000000000000000000000000000
  manifest:    4:d01b250baaa05909152f7ae07d7a649deea0df9a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       a
  extra:       branch=foo
  description:
  modify a branch
  
  
  $ rm $branchcache
  $ echo corrupted > $branchcache

  $ hg log -qr foo
  4:98d14f698afe

  $ cat $branchcache
  98d14f698afeaff8cb612dcf215ce95e639effc3 4
  9d567d0b51f9e2068b054e1948e1a927f99b5874 default
  98d14f698afeaff8cb612dcf215ce95e639effc3 foo
  ed2bbf4e01029020711be82ca905283e883f0e11 bar

Push should update the branch cache:

  $ hg init ../target

Pushing just rev 0:

  $ hg push -qr 0 ../target

  $ cat ../target/$branchcache
  db01e8ea3388fd3c7c94e1436ea2bd6a53d581c5 0
  db01e8ea3388fd3c7c94e1436ea2bd6a53d581c5 default

Pushing everything:

  $ hg push -qf ../target

  $ cat ../target/$branchcache
  98d14f698afeaff8cb612dcf215ce95e639effc3 4
  9d567d0b51f9e2068b054e1948e1a927f99b5874 default
  98d14f698afeaff8cb612dcf215ce95e639effc3 foo
  ed2bbf4e01029020711be82ca905283e883f0e11 bar

Update with no arguments: tipmost revision of the current branch:

  $ hg up -q -C 0
  $ hg up -q
  $ hg id
  9d567d0b51f9

  $ hg up -q 1
  $ hg up -q
  $ hg id
  98d14f698afe (foo) tip

  $ hg branch foobar
  marked working directory as branch foobar

  $ hg up
  abort: branch foobar not found
  [255]

Fastforward merge:

  $ hg branch ff
  marked working directory as branch ff

  $ echo ff > ff
  $ hg ci -Am'fast forward'
  adding ff

  $ hg up foo
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved

  $ hg merge ff
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg branch
  foo
  $ hg commit -m'Merge ff into foo'
  $ hg parents
  changeset:   6:917eb54e1b4b
  branch:      foo
  tag:         tip
  parent:      4:98d14f698afe
  parent:      5:6683a60370cb
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Merge ff into foo
  
  $ hg manifest
  a
  ff


Test merging, add 3 default heads and one test head:

  $ cd ..
  $ hg init merges
  $ cd merges
  $ echo a > a
  $ hg ci -Ama
  adding a

  $ echo b > b
  $ hg ci -Amb
  adding b

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -Amc
  adding c
  created new head

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo d > d
  $ hg ci -Amd
  adding d
  created new head

  $ hg up 0
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg branch test
  marked working directory as branch test
  $ echo e >> e
  $ hg ci -Ame
  adding e

  $ hg log
  changeset:   4:3a1e01ed1df4
  branch:      test
  tag:         tip
  parent:      0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     e
  
  changeset:   3:980f7dc84c29
  parent:      0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     d
  
  changeset:   2:d36c0562f908
  parent:      0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     c
  
  changeset:   1:d2ae7f538514
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     b
  
  changeset:   0:cb9a9f314b8b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     a
  
Implicit merge with test branch as parent:

  $ hg merge
  abort: branch 'test' has one head - please merge with an explicit rev
  (run 'hg heads' to see all heads)
  [255]
  $ hg up -C default
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Implicit merge with default branch as parent:

  $ hg merge
  abort: branch 'default' has 3 heads - please merge with an explicit rev
  (run 'hg heads .' to see heads)
  [255]

3 branch heads, explicit merge required:

  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m merge

2 branch heads, implicit merge works:

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

