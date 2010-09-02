  $ hg init

  $ echo foo > a
  $ echo foo > b
  $ hg add a b

  $ hg ci -m "test"

  $ echo blah > a

  $ hg ci -m "branch a"

  $ hg co 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo blah > b

  $ hg ci -m "branch b"
  created new head
  $ HGMERGE=true hg merge 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m "merge b/a -> blah"

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGMERGE=true hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge a/b -> blah"
  created new head

  $ hg log
  changeset:   4:2ee31f665a86
  tag:         tip
  parent:      1:96155394af80
  parent:      2:92cc4c306b19
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge a/b -> blah
  
  changeset:   3:e16a66a37edd
  parent:      2:92cc4c306b19
  parent:      1:96155394af80
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge b/a -> blah
  
  changeset:   2:92cc4c306b19
  parent:      0:5e0375449e74
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     branch b
  
  changeset:   1:96155394af80
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     branch a
  
  changeset:   0:5e0375449e74
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  
  $ hg debugindex .hg/store/00changelog.i
     rev    offset  length   base linkrev nodeid       p1           p2
       0         0      60      0       0 5e0375449e74 000000000000 000000000000
       1        60      62      1       1 96155394af80 5e0375449e74 000000000000
       2       122      62      2       2 92cc4c306b19 5e0375449e74 000000000000
       3       184      69      3       3 e16a66a37edd 92cc4c306b19 96155394af80
       4       253      29      3       4 2ee31f665a86 96155394af80 92cc4c306b19

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
