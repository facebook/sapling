#chg-compatible
#debugruntest-compatible

  $ setconfig workingcopy.ruststatus=False
  $ disable treemanifest
  $ hg init

  $ echo foo > a
  $ echo foo > b
  $ hg add a b

  $ hg ci -m "test"

  $ echo blah > a

  $ hg ci -m "branch a"

  $ hg co 'desc(test)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo blah > b

  $ hg ci -m "branch b"
  $ HGMERGE=true hg merge 96155394af80e900c1e01da6607cb913696d5782
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg ci -m "merge b/a -> blah"

  $ hg co 96155394af80e900c1e01da6607cb913696d5782
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ HGMERGE=true hg merge 'max(desc(branch))'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m "merge a/b -> blah"

  $ hg log
  commit:      2ee31f665a86
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge a/b -> blah
  
  commit:      e16a66a37edd
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     merge b/a -> blah
  
  commit:      92cc4c306b19
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     branch b
  
  commit:      96155394af80
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     branch a
  
  commit:      5e0375449e74
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test
  
  $ hg debugindex --changelog
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0      60  .....       0 5e0375449e74 000000000000 000000000000 (re)
       1        60      62  .....       1 96155394af80 5e0375449e74 000000000000 (re)
       2       122      62  .....       2 92cc4c306b19 5e0375449e74 000000000000 (re)
       3       184      69  .....       3 e16a66a37edd 92cc4c306b19 96155394af80 (re)
       4       253      69  .....       4 2ee31f665a86 96155394af80 92cc4c306b19 (re)

revision 1
  $ hg manifest --debug 96155394af80e900c1e01da6607cb913696d5782
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   b
revision 2
  $ hg manifest --debug 'max(desc(branch))'
  2ed2a3912a0b24502043eae84ee4b279c18b90dd 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b
revision 3
  $ hg manifest --debug e16a66a37edd20d849a93a9fb61e157d717fac36
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b
revision 4
  $ hg manifest --debug 'max(desc(merge))'
  79d7492df40aa0fa093ec4209be78043c181f094 644   a
  79d7492df40aa0fa093ec4209be78043c181f094 644   b

  $ hg debugindex a
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       5  .....       0 2ed2a3912a0b 000000000000 000000000000 (re)
       1         5       6  .....       1 79d7492df40a 2ed2a3912a0b 000000000000 (re)

  $ hg verify
  warning: verify does not actually check anything in this repo
