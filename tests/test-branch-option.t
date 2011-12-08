test branch selection options

  $ hg init branch
  $ cd branch
  $ hg branch a
  marked working directory as branch a
  (branches are permanent and global, did you want a bookmark?)
  $ echo a > foo
  $ hg ci -d '0 0' -Ama
  adding foo
  $ echo a2 > foo
  $ hg ci -d '0 0' -ma2
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch c
  marked working directory as branch c
  (branches are permanent and global, did you want a bookmark?)
  $ echo c > foo
  $ hg ci -d '0 0' -mc
  $ hg tag -l z
  $ cd ..
  $ hg clone -r 0 branch branch2
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  updating to branch a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd branch2
  $ hg up 0
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > foo
  $ hg ci -d '0 0' -mb
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --encoding utf-8 branch æ
  marked working directory as branch \xc3\xa6 (esc)
  (branches are permanent and global, did you want a bookmark?)
  $ echo ae1 > foo
  $ hg ci -d '0 0' -mae1
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --encoding utf-8 branch -f æ
  marked working directory as branch \xc3\xa6 (esc)
  (branches are permanent and global, did you want a bookmark?)
  $ echo ae2 > foo
  $ hg ci -d '0 0' -mae2
  created new head
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg branch -f b
  marked working directory as branch b
  (branches are permanent and global, did you want a bookmark?)
  $ echo b2 > foo
  $ hg ci -d '0 0' -mb2
  created new head

unknown branch and fallback

  $ hg in -qbz
  abort: unknown branch 'z'!
  [255]
  $ hg in -q ../branch#z
  2:f25d57ab0566
  $ hg out -qbz
  abort: unknown branch 'z'!
  [255]

in rev c branch a

  $ hg in -qr c ../branch#a
  1:dd6e60a716c6
  2:f25d57ab0566
  $ hg in -qr c -b a
  1:dd6e60a716c6
  2:f25d57ab0566

out branch .

  $ hg out -q ../branch#.
  1:b84708d77ab7
  4:65511d0e2b55
  $ hg out -q -b .
  1:b84708d77ab7
  4:65511d0e2b55

out branch . non-ascii

  $ hg --encoding utf-8 up æ
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --encoding latin1 out -q ../branch#.
  2:df5a44224d4e
  3:4f4a5125ca10
  $ hg --encoding latin1 out -q -b .
  2:df5a44224d4e
  3:4f4a5125ca10

clone branch b

  $ cd ..
  $ hg clone branch2#b branch3
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files (+1 heads)
  updating to branch b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -q -R branch3 heads b
  2:65511d0e2b55
  1:b84708d77ab7
  $ hg -q -R branch3 parents
  2:65511d0e2b55
  $ rm -rf branch3

clone rev a branch b

  $ hg clone -r a branch2#b branch3
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 1 files (+1 heads)
  updating to branch a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -q -R branch3 heads b
  2:65511d0e2b55
  1:b84708d77ab7
  $ hg -q -R branch3 parents
  0:5b65ba7c951d
  $ rm -rf branch3
