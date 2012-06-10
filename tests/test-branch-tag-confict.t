Initial setup.

  $ hg init repo
  $ cd repo
  $ touch thefile
  $ hg ci -A -m 'Initial commit.'
  adding thefile

Create a tag.

  $ hg tag branchortag

Create a branch with the same name as the tag.

  $ hg branch branchortag
  marked working directory as branch branchortag
  (branches are permanent and global, did you want a bookmark?)
  $ hg ci -m 'Create a branch with the same name as a tag.'

This is what we have:

  $ hg log
  changeset:   2:10519b3f489a
  branch:      branchortag
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Create a branch with the same name as a tag.
  
  changeset:   1:2635c45ca99b
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Added tag branchortag for changeset f57387372b5d
  
  changeset:   0:f57387372b5d
  tag:         branchortag
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Initial commit.
  
Update to the tag:

  $ hg up 'tag(branchortag)'
  0 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg parents
  changeset:   0:f57387372b5d
  tag:         branchortag
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Initial commit.
  
Updating to the branch:

  $ hg up 'branch(branchortag)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg parents
  changeset:   2:10519b3f489a
  branch:      branchortag
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     Create a branch with the same name as a tag.
  

  $ cd ..
