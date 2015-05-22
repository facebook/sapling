
  $ cat <<EOF >> $HGRCPATH
  > [extensions]
  > convert =
  > [convert]
  > hg.tagsbranch = 0
  > EOF
  $ hg init source
  $ cd source
  $ echo a > a
  $ hg ci -qAm adda

Add a merge with one parent in the same branch

  $ echo a >> a
  $ hg ci -qAm changea
  $ hg up -qC 0
  $ hg branch branch0
  marked working directory as branch branch0
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > b
  $ hg ci -qAm addb
  $ hg up -qC
  $ hg merge default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -qm mergeab
  $ hg tag -ql mergeab
  $ cd ..

Miss perl... sometimes

  $ cat > filter.py <<EOF
  > import sys, re
  > 
  > r = re.compile(r'^(?:\d+|pulling from)')
  > sys.stdout.writelines([l for l in sys.stdin if r.search(l)])
  > EOF

convert

  $ hg convert -v --config convert.hg.clonebranches=1 source dest |
  >     python filter.py
  3 adda
  2 changea
  1 addb
  pulling from default into branch0
  1 changesets found
  0 mergeab
  pulling from default into branch0
  1 changesets found

Add a merge with both parents and child in different branches

  $ cd source
  $ hg branch branch1
  marked working directory as branch branch1
  $ echo a > file1
  $ hg ci -qAm c1
  $ hg up -qC mergeab
  $ hg branch branch2
  marked working directory as branch branch2
  $ echo a > file2
  $ hg ci -qAm c2
  $ hg merge branch1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg branch branch3
  marked working directory as branch branch3
  $ hg ci -qAm c3
  $ cd ..

incremental conversion

  $ hg convert -v --config convert.hg.clonebranches=1 source dest |
  >     python filter.py
  2 c1
  pulling from branch0 into branch1
  4 changesets found
  1 c2
  pulling from branch0 into branch2
  4 changesets found
  0 c3
  pulling from branch1 into branch3
  5 changesets found
  pulling from branch2 into branch3
  1 changesets found
