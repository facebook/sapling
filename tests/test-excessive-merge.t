  $ hg init

  $ echo foo > a
  $ echo foo > b
  $ hg add a b

  $ hg ci -m "test" -d "1000000 0"

  $ echo blah > a

  $ hg ci -m "branch a" -d "1000000 0"

  $ hg co 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo blah > b

  $ hg ci -m "branch b" -d "1000000 0"
  created new head
  $ HGMERGE=true hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m "merge b/a -> blah" -d "1000000 0"

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGMERGE=true hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge a/b -> blah" -d "1000000 0"
  created new head

  $ hg log
  changeset:   4:f6c172c6198c
  tag:         tip
  parent:      1:448a8c5e42f1
  parent:      2:7c5dc2e857f2
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     merge a/b -> blah
  
  changeset:   3:13d875a22764
  parent:      2:7c5dc2e857f2
  parent:      1:448a8c5e42f1
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     merge b/a -> blah
  
  changeset:   2:7c5dc2e857f2
  parent:      0:dc1751ec2e9d
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     branch b
  
  changeset:   1:448a8c5e42f1
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     branch a
  
  changeset:   0:dc1751ec2e9d
  user:        test
  date:        Mon Jan 12 13:46:40 1970 +0000
  summary:     test
  
  $ hg debugindex .hg/store/00changelog.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      64      0       0 dc1751ec2e9d 000000000000 000000000000
       1        64      68      1       1 448a8c5e42f1 dc1751ec2e9d 000000000000
       2       132      68      2       2 7c5dc2e857f2 dc1751ec2e9d 000000000000
       3       200      75      3       3 13d875a22764 7c5dc2e857f2 448a8c5e42f1
       4       275      29      3       4 f6c172c6198c 448a8c5e42f1 7c5dc2e857f2

revision 1
  $ hg manifest --debug 1
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   b
revision 2
  $ hg manifest --debug 2
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b
revision 3
  $ hg manifest --debug 3
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b
revision 4
  $ hg manifest --debug 4
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b

  $ hg debugindex .hg/store/data/a.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0       5      0       0 2ed2a3912a0b 000000000000 000000000000
       1         5       6      1       1 79d7492df40a 2ed2a3912a0b 000000000000

  $ hg verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  2 files, 5 changesets, 4 total revisions
