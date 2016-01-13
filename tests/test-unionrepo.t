Test unionrepo functionality

Create one repository

  $ hg init repo1
  $ cd repo1
  $ touch repo1-0
  $ echo repo1-0 > f
  $ hg ci -Aqmrepo1-0
  $ touch repo1-1
  $ echo repo1-1 >> f
  $ hg ci -Aqmrepo1-1
  $ touch repo1-2
  $ echo repo1-2 >> f
  $ hg ci -Aqmrepo1-2
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  2:68c0685446a3  repo1-2
  1:8a58db72e69d  repo1-1
  0:f093fec0529b  repo1-0
  $ tip1=`hg id -q`
  $ cd ..

- and a clone with a not-completely-trivial history

  $ hg clone -q repo1 --rev 0 repo2
  $ cd repo2
  $ touch repo2-1
  $ sed '1i\
  > repo2-1 at top
  > ' f > f.tmp
  $ mv f.tmp f
  $ hg ci -Aqmrepo2-1
  $ touch repo2-2
  $ hg pull -q ../repo1 -r 1
  $ hg merge -q
  $ hg ci -Aqmrepo2-2-merge
  $ touch repo2-3
  $ echo repo2-3 >> f
  $ hg ci -mrepo2-3
  $ hg log --template '{rev}:{node|short}  {desc|firstline}\n'
  4:2f0d178c469c  repo2-3
  3:9e6fb3e0b9da  repo2-2-merge
  2:8a58db72e69d  repo1-1
  1:c337dba826e7  repo2-1
  0:f093fec0529b  repo1-0
  $ cd ..

revisions from repo2 appear as appended / pulled to repo1

  $ hg -R union:repo1+repo2 log --template '{rev}:{node|short}  {desc|firstline}\n'
  5:2f0d178c469c  repo2-3
  4:9e6fb3e0b9da  repo2-2-merge
  3:c337dba826e7  repo2-1
  2:68c0685446a3  repo1-2
  1:8a58db72e69d  repo1-1
  0:f093fec0529b  repo1-0

manifest can be retrieved for revisions in both repos

  $ hg -R union:repo1+repo2 mani -r $tip1
  f
  repo1-0
  repo1-1
  repo1-2
  $ hg -R union:repo1+repo2 mani -r 4
  f
  repo1-0
  repo1-1
  repo2-1
  repo2-2

files can be retrieved form both repos

  $ hg -R repo1 cat repo1/f -r2
  repo1-0
  repo1-1
  repo1-2

  $ hg -R union:repo1+repo2 cat -r$tip1 repo1/f
  repo1-0
  repo1-1
  repo1-2

  $ hg -R union:repo1+repo2 cat -r4 $TESTTMP/repo1/f
  repo2-1 at top
  repo1-0
  repo1-1

files can be compared across repos

  $ hg -R union:repo1+repo2 diff -r$tip1 -rtip
  diff -r 68c0685446a3 -r 2f0d178c469c f
  --- a/f	Thu Jan 01 00:00:00 1970 +0000
  +++ b/f	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,3 +1,4 @@
  +repo2-1 at top
   repo1-0
   repo1-1
  -repo1-2
  +repo2-3

heads from both repos are found correctly

  $ hg -R union:repo1+repo2 heads --template '{rev}:{node|short}  {desc|firstline}\n'
  5:2f0d178c469c  repo2-3
  2:68c0685446a3  repo1-2

revsets works across repos

  $ hg -R union:repo1+repo2 id -r "ancestor($tip1, 5)"
  8a58db72e69d

annotate works - an indication that linkrevs works

  $ hg --cwd repo1 -Runion:../repo2 annotate $TESTTMP/repo1/f -r tip
  3: repo2-1 at top
  0: repo1-0
  1: repo1-1
  5: repo2-3

union repos can be cloned ... and clones works correctly

  $ hg clone -U union:repo1+repo2 repo3
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 6 changesets with 11 changes to 6 files (+1 heads)

  $ hg -R repo3 paths
  default = union:repo1+repo2

  $ hg -R repo3 verify
  checking changesets
  checking manifests
  crosschecking files in changesets and manifests
  checking files
  6 files, 6 changesets, 11 total revisions

  $ hg -R repo3 heads --template '{rev}:{node|short}  {desc|firstline}\n'
  5:2f0d178c469c  repo2-3
  2:68c0685446a3  repo1-2

  $ hg -R repo3 log --template '{rev}:{node|short}  {desc|firstline}\n'
  5:2f0d178c469c  repo2-3
  4:9e6fb3e0b9da  repo2-2-merge
  3:c337dba826e7  repo2-1
  2:68c0685446a3  repo1-2
  1:8a58db72e69d  repo1-1
  0:f093fec0529b  repo1-0

union repos should use the correct rev number (issue5024)

  $ hg init a
  $ cd a
  $ echo a0 >> f
  $ hg ci -Aqm a0
  $ cd ..
  $ hg init b
  $ cd b
  $ echo b0 >> f
  $ hg ci -Aqm b0
  $ echo b1 >> f
  $ hg ci -qm b1
  $ cd ..

"hg files -v" to call fctx.size() -> fctx.iscensored()
  $ hg files -R union:b+a -r2 -v
           3   b/f (glob)
