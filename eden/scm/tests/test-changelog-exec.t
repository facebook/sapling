#chg-compatible
#debugruntest-compatible

#require execbit

b51a8138292a introduced a regression where we would mention in the
changelog executable files added by the second parent of a merge. Test
that that doesn't happen anymore

  $ hg init repo
  $ cd repo
  $ echo foo > foo
  $ hg ci -qAm 'add foo'

  $ echo bar > bar
  $ chmod +x bar
  $ hg ci -qAm 'add bar'

manifest of p2:

  $ hg manifest
  bar
  foo

  $ hg up -qC bbd179dfa0a71671c253b3ae0aa1513b60d199fa
  $ echo >> foo
  $ hg ci -m 'change foo'

manifest of p1:

  $ hg manifest
  foo

  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ chmod +x foo
  $ hg ci -m 'merge'

this should not mention bar but should mention foo:

  $ hg tip -v
  commit:      c53d17ff3380
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       foo
  description:
  merge
  
  

  $ hg debugindex bar
     rev    offset  length  ..... linkrev nodeid       p1           p2 (re)
       0         0       5  .....       1 b004912a8510 000000000000 000000000000 (re)

  $ cd ..
