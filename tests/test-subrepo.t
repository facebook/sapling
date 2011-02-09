  $ rm -rf sub
  $ mkdir sub
  $ cd sub
  $ hg init t
  $ cd t

first revision, no sub

  $ echo a > a
  $ hg ci -Am0
  adding a

add first sub

  $ echo s = s > .hgsub
  $ hg add .hgsub
  $ hg init s
  $ echo a > s/a

Issue2232: committing a subrepo without .hgsub

  $ hg ci -mbad s
  abort: can't commit subrepos without .hgsub
  [255]

  $ hg -R s ci -Ams0
  adding a
  $ hg sum
  parent: 0:f7b1eb17ad24 tip
   0
  branch: default
  commit: 1 added, 1 subrepos
  update: (current)
  $ hg ci -m1
  committing subrepository s

Issue2022: update -C

  $ echo b > s/a
  $ hg sum
  parent: 1:7cf8cfea66e4 tip
   1
  branch: default
  commit: 1 subrepos
  update: (current)
  $ hg co -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sum
  parent: 1:7cf8cfea66e4 tip
   1
  branch: default
  commit: (clean)
  update: (current)

add sub sub

  $ echo ss = ss > s/.hgsub
  $ hg init s/ss
  $ echo a > s/ss/a
  $ hg -R s add s/.hgsub
  $ hg -R s/ss add s/ss/a
  $ hg sum
  parent: 1:7cf8cfea66e4 tip
   1
  branch: default
  commit: 1 subrepos
  update: (current)
  $ hg ci -m2
  committing subrepository s
  committing subrepository s/ss
  $ hg sum
  parent: 2:df30734270ae tip
   2
  branch: default
  commit: (clean)
  update: (current)

bump sub rev (and check it is ignored by ui.commitsubrepos)

  $ echo b > s/a
  $ hg -R s ci -ms1
  $ hg --config ui.commitsubrepos=no ci -m3
  committing subrepository s

leave sub dirty (and check ui.commitsubrepos=no aborts the commit)

  $ echo c > s/a
  $ hg --config ui.commitsubrepos=no ci -m4
  abort: uncommitted changes in subrepo s
  [255]
  $ hg ci -m4
  committing subrepository s
  $ hg tip -R s
  changeset:   3:1c833a7a9e3a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     4
  

check caching

  $ hg co 0
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ hg debugsub

restore

  $ hg co
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg debugsub
  path s
   source   s
   revision 1c833a7a9e3a4445c711aaf0f012379cd0d4034e

new branch for merge tests

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo t = t >> .hgsub
  $ hg init t
  $ echo t > t/t
  $ hg -R t add t
  adding t/t

5

  $ hg ci -m5 # add sub
  committing subrepository t
  created new head
  $ echo t2 > t/t

6

  $ hg st -R s
  $ hg ci -m6 # change sub
  committing subrepository t
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  path t
   source   t
   revision 6747d179aa9a688023c4b0cad32e4c92bb7f34ad
  $ echo t3 > t/t

7

  $ hg ci -m7 # change sub again for conflict test
  committing subrepository t
  $ hg rm .hgsub

8

  $ hg ci -m8 # remove sub

merge tests

  $ hg co -C 3
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 5 # test adding
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg debugsub
  path s
   source   s
   revision fc627a69481fcbe5f1135069e8a3881c023e4cf5
  path t
   source   t
   revision 60ca1237c19474e7a3978b0dc1ca4e6f36d51382
  $ hg ci -m9
  created new head
  $ hg merge 6 --debug # test change
    searching for copies back to rev 2
  resolving manifests
   overwrite None partial False
   ancestor 1f14a2e2d3ec local f0d2028bf86d+ remote 1831e14459c4
   .hgsubstate: versions differ -> m
  updating: .hgsubstate 1/1 files (100.00%)
  subrepo merge f0d2028bf86d+ 1831e14459c4 1f14a2e2d3ec
    subrepo t: other changed, get t:6747d179aa9a688023c4b0cad32e4c92bb7f34ad:hg
  getting subrepo t
  resolving manifests
   overwrite True partial False
   ancestor 60ca1237c194+ local 60ca1237c194+ remote 6747d179aa9a
   t: remote is newer -> g
  updating: t 1/1 files (100.00%)
  getting t
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg debugsub
  path s
   source   s
   revision fc627a69481fcbe5f1135069e8a3881c023e4cf5
  path t
   source   t
   revision 6747d179aa9a688023c4b0cad32e4c92bb7f34ad
  $ echo conflict > t/t
  $ hg ci -m10
  committing subrepository t
  $ HGMERGE=internal:merge hg merge --debug 7 # test conflict
    searching for copies back to rev 2
  resolving manifests
   overwrite None partial False
   ancestor 1831e14459c4 local e45c8b14af55+ remote f94576341bcf
   .hgsubstate: versions differ -> m
  updating: .hgsubstate 1/1 files (100.00%)
  subrepo merge e45c8b14af55+ f94576341bcf 1831e14459c4
    subrepo t: both sides changed, merge with t:7af322bc1198a32402fe903e0b7ebcfc5c9bf8f4:hg
  merging subrepo t
    searching for copies back to rev 2
  resolving manifests
   overwrite None partial False
   ancestor 6747d179aa9a local 20a0db6fbf6c+ remote 7af322bc1198
   t: versions differ -> m
  preserving t for resolve of t
  updating: t 1/1 files (100.00%)
  picked tool 'internal:merge' for t (binary False symlink False)
  merging t
  my t@20a0db6fbf6c+ other t@7af322bc1198 ancestor t@6747d179aa9a
  warning: conflicts during merge.
  merging t failed!
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

should conflict

  $ cat t/t
  <<<<<<< local
  conflict
  =======
  t3
  >>>>>>> other

clone

  $ cd ..
  $ hg clone t tc
  updating to branch default
  pulling subrepo s from $TESTTMP/sub/t/s
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 5 changes to 3 files
  pulling subrepo s/ss from $TESTTMP/sub/t/s/ss
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  pulling subrepo t from $TESTTMP/sub/t/t
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 4 changesets with 4 changes to 1 files (+1 heads)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd tc
  $ hg debugsub
  path s
   source   s
   revision fc627a69481fcbe5f1135069e8a3881c023e4cf5
  path t
   source   t
   revision 20a0db6fbf6c3d2836e6519a642ae929bfc67c0e

push

  $ echo bah > t/t
  $ hg ci -m11
  committing subrepository t
  $ hg push
  pushing to $TESTTMP/sub/t
  pushing subrepo s/ss to $TESTTMP/sub/t/s/ss
  searching for changes
  no changes found
  pushing subrepo s to $TESTTMP/sub/t/s
  searching for changes
  no changes found
  pushing subrepo t to $TESTTMP/sub/t/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

push -f

  $ echo bah > s/a
  $ hg ci -m12
  committing subrepository s
  $ hg push
  pushing to $TESTTMP/sub/t
  pushing subrepo s/ss to $TESTTMP/sub/t/s/ss
  searching for changes
  no changes found
  pushing subrepo s to $TESTTMP/sub/t/s
  searching for changes
  abort: push creates new remote heads on branch 'default'!
  (did you forget to merge? use push -f to force)
  [255]
  $ hg push -f
  pushing to $TESTTMP/sub/t
  pushing subrepo s/ss to $TESTTMP/sub/t/s/ss
  searching for changes
  no changes found
  pushing subrepo s to $TESTTMP/sub/t/s
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  pushing subrepo t to $TESTTMP/sub/t/t
  searching for changes
  no changes found
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

update

  $ cd ../t
  $ hg up -C # discard our earlier merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo blah > t/t
  $ hg ci -m13
  committing subrepository t

pull

  $ cd ../tc
  $ hg pull
  pulling from $TESTTMP/sub/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

should pull t

  $ hg up
  pulling subrepo t from $TESTTMP/sub/t/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat t/t
  blah

bogus subrepo path aborts

  $ echo 'bogus=[boguspath' >> .hgsub
  $ hg ci -m 'bogus subrepo path'
  abort: missing ] in subrepo source
  [255]

Issue1986: merge aborts when trying to merge a subrepo that
shouldn't need merging

# subrepo layout
#
#   o   5 br
#  /|
# o |   4 default
# | |
# | o   3 br
# |/|
# o |   2 default
# | |
# | o   1 br
# |/
# o     0 default

  $ cd ..
  $ rm -rf sub
  $ hg init main
  $ cd main
  $ hg init s
  $ cd s
  $ echo a > a
  $ hg ci -Am1
  adding a
  $ hg branch br
  marked working directory as branch br
  $ echo a >> a
  $ hg ci -m1
  $ hg up default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am1
  adding b
  $ hg up br
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m1
  $ hg up 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -Am1
  adding c
  $ hg up 3
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m1

# main repo layout:
#
#   * <-- try to merge default into br again
# .`|
# . o   5 br      --> substate = 5
# . |
# o |   4 default --> substate = 4
# | |
# | o   3 br      --> substate = 2
# |/|
# o |   2 default --> substate = 2
# | |
# | o   1 br      --> substate = 3
# |/
# o     0 default --> substate = 2

  $ cd ..
  $ echo 's = s' > .hgsub
  $ hg -R s up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg ci -Am1
  adding .hgsub
  committing subrepository s
  $ hg branch br
  marked working directory as branch br
  $ echo b > b
  $ hg -R s up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg ci -Am1
  adding b
  committing subrepository s
  $ hg up default
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo c > c
  $ hg ci -Am1
  adding c
  $ hg up 1
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg ci -m1
  $ hg up 2
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg -R s up 4
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo d > d
  $ hg ci -Am1
  adding d
  committing subrepository s
  $ hg up 3
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg -R s up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo e > e
  $ hg ci -Am1
  adding e
  committing subrepository s

  $ hg up 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 4    # try to merge default into br again
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cd ..

test subrepo delete from .hgsubstate

  $ hg init testdelete
  $ mkdir testdelete/nested testdelete/nested2
  $ hg init testdelete/nested
  $ hg init testdelete/nested2
  $ echo test > testdelete/nested/foo
  $ echo test > testdelete/nested2/foo
  $ hg -R testdelete/nested add
  adding testdelete/nested/foo
  $ hg -R testdelete/nested2 add
  adding testdelete/nested2/foo
  $ hg -R testdelete/nested ci -m test
  $ hg -R testdelete/nested2 ci -m test
  $ echo nested = nested > testdelete/.hgsub
  $ echo nested2 = nested2 >> testdelete/.hgsub
  $ hg -R testdelete add
  adding testdelete/.hgsub
  $ hg -R testdelete ci -m "nested 1 & 2 added"
  committing subrepository nested
  committing subrepository nested2
  $ echo nested = nested > testdelete/.hgsub
  $ hg -R testdelete ci -m "nested 2 deleted"
  $ cat testdelete/.hgsubstate
  bdf5c9a3103743d900b12ae0db3ffdcfd7b0d878 nested
  $ hg -R testdelete remove testdelete/.hgsub
  $ hg -R testdelete ci -m ".hgsub deleted"
  $ cat testdelete/.hgsubstate

test repository cloning

  $ mkdir mercurial mercurial2
  $ hg init nested_absolute
  $ echo test > nested_absolute/foo
  $ hg -R nested_absolute add
  adding nested_absolute/foo
  $ hg -R nested_absolute ci -mtest
  $ cd mercurial
  $ hg init nested_relative
  $ echo test2 > nested_relative/foo2
  $ hg -R nested_relative add
  adding nested_relative/foo2
  $ hg -R nested_relative ci -mtest2
  $ hg init main
  $ echo "nested_relative = ../nested_relative" > main/.hgsub
  $ echo "nested_absolute = `pwd`/nested_absolute" >> main/.hgsub
  $ hg -R main add
  adding main/.hgsub
  $ hg -R main ci -m "add subrepos"
  committing subrepository nested_absolute
  committing subrepository nested_relative
  $ cd ..
  $ hg clone mercurial/main mercurial2/main
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat mercurial2/main/nested_absolute/.hg/hgrc \
  >     mercurial2/main/nested_relative/.hg/hgrc
  [paths]
  default = $TESTTMP/sub/mercurial/nested_absolute
  [paths]
  default = $TESTTMP/sub/mercurial/nested_relative
  $ rm -rf mercurial mercurial2

Issue1977: multirepo push should fail if subrepo push fails

  $ hg init repo
  $ hg init repo/s
  $ echo a > repo/s/a
  $ hg -R repo/s ci -Am0
  adding a
  $ echo s = s > repo/.hgsub
  $ hg -R repo ci -Am1
  adding .hgsub
  committing subrepository s
  $ hg clone repo repo2
  updating to branch default
  pulling subrepo s from $TESTTMP/sub/repo/s
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -q -R repo2 pull -u
  $ echo 1 > repo2/s/a
  $ hg -R repo2/s ci -m2
  $ hg -q -R repo2/s push
  $ hg -R repo2/s up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 2 > repo2/s/a
  $ hg -R repo2/s ci -m3
  created new head
  $ hg -R repo2 ci -m3
  committing subrepository s
  $ hg -q -R repo2 push
  abort: push creates new remote heads on branch 'default'!
  (did you forget to merge? use push -f to force)
  [255]
  $ hg -R repo update
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ rm -rf repo2 repo


Issue1852 subrepos with relative paths always push/pull relative to default

Prepare a repo with subrepo

  $ hg init issue1852a
  $ cd issue1852a
  $ hg init sub/repo
  $ echo test > sub/repo/foo
  $ hg -R sub/repo add sub/repo/foo
  $ echo sub/repo = sub/repo > .hgsub
  $ hg add .hgsub
  $ hg ci -mtest
  committing subrepository sub/repo
  $ echo test >> sub/repo/foo
  $ hg ci -mtest
  committing subrepository sub/repo
  $ cd ..

Create repo without default path, pull top repo, and see what happens on update

  $ hg init issue1852b
  $ hg -R issue1852b pull issue1852a
  pulling from issue1852a
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 3 changes to 2 files
  (run 'hg update' to get a working copy)
  $ hg -R issue1852b update
  abort: default path for subrepository sub/repo not found
  [255]

Pull -u now doesn't help

  $ hg -R issue1852b pull -u issue1852a
  pulling from issue1852a
  searching for changes
  no changes found

Try the same, but with pull -u

  $ hg init issue1852c
  $ hg -R issue1852c pull -r0 -u issue1852a
  pulling from issue1852a
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
  pulling subrepo sub/repo from issue1852a/sub/repo
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 1 files
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Try to push from the other side

  $ hg -R issue1852a push `pwd`/issue1852c
  pushing to $TESTTMP/sub/issue1852c
  pushing subrepo sub/repo to $TESTTMP/sub/issue1852c/sub/repo
  searching for changes
  no changes found
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Check status of files when none of them belong to the first
subrepository:

  $ hg init subrepo-status
  $ cd subrepo-status
  $ hg init subrepo-1
  $ hg init subrepo-2
  $ cd subrepo-2
  $ touch file
  $ hg add file
  $ cd ..
  $ echo subrepo-1 = subrepo-1 > .hgsub
  $ echo subrepo-2 = subrepo-2 >> .hgsub
  $ hg add .hgsub
  $ hg ci -m 'Added subrepos'
  committing subrepository subrepo-1
  committing subrepository subrepo-2
  $ hg st subrepo-2/file

Check hg update --clean
  $ cd $TESTTMP/sub/t
  $ rm -r t/t.orig
  $ hg status -S --all
  C .hgsub
  C .hgsubstate
  C a
  C s/.hgsub
  C s/.hgsubstate
  C s/a
  C s/ss/a
  C t/t
  $ echo c1 > s/a
  $ cd s
  $ echo c1 > b
  $ echo c1 > c
  $ hg add b
  $ cd ..
  $ hg status -S
  M s/a
  A s/b
  ? s/c
  $ hg update -C
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg status -S
  ? s/b
  ? s/c

Sticky subrepositories, no changes
  $ cd $TESTTMP/sub/t
  $ hg id
  925c17564ef8 tip
  $ hg -R s id
  12a213df6fa9 tip
  $ hg -R t id  
  52c0adc0515a tip
  $ hg update 11
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  365661e5936a
  $ hg -R s id
  fc627a69481f
  $ hg -R t id  
  e95bcfa18a35

Sticky subrepositorys, file changes
  $ touch s/f1
  $ touch t/f1
  $ hg add -S s/f1
  $ hg add -S t/f1
  $ hg id
  365661e5936a
  $ hg -R s id
  fc627a69481f+
  $ hg -R t id  
  e95bcfa18a35+
  $ hg update tip
   subrepository sources for s differ
  use (l)ocal source (fc627a69481f) or (r)emote source (12a213df6fa9)?
   l
   subrepository sources for t differ
  use (l)ocal source (e95bcfa18a35) or (r)emote source (52c0adc0515a)?
   l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  925c17564ef8+ tip
  $ hg -R s id
  fc627a69481f+
  $ hg -R t id  
  e95bcfa18a35+
  $ hg update --clean tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Sticky subrepository, revision updates
  $ hg id
  925c17564ef8 tip
  $ hg -R s id
  12a213df6fa9 tip
  $ hg -R t id  
  52c0adc0515a tip
  $ cd s
  $ hg update -r -2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ../t
  $ hg update -r 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
  $ hg update 10
   subrepository sources for t differ (in checked out version)
  use (l)ocal source (7af322bc1198) or (r)emote source (20a0db6fbf6c)?
   l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  e45c8b14af55+
  $ hg -R s id
  1c833a7a9e3a
  $ hg -R t id  
  7af322bc1198

Sticky subrepository, file changes and revision updates
  $ touch s/f1
  $ touch t/f1
  $ hg add -S s/f1
  $ hg add -S t/f1
  $ hg id
  e45c8b14af55+
  $ hg -R s id
  1c833a7a9e3a+
  $ hg -R t id  
  7af322bc1198+
  $ hg update tip
   subrepository sources for s differ
  use (l)ocal source (1c833a7a9e3a) or (r)emote source (12a213df6fa9)?
   l
   subrepository sources for t differ
  use (l)ocal source (7af322bc1198) or (r)emote source (52c0adc0515a)?
   l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  925c17564ef8 tip
  $ hg -R s id
  1c833a7a9e3a+
  $ hg -R t id  
  7af322bc1198+

Sticky repository, update --clean
  $ hg update --clean tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  925c17564ef8 tip
  $ hg -R s id
  12a213df6fa9 tip
  $ hg -R t id  
  52c0adc0515a tip

Test subrepo already at intended revision:
  $ cd s
  $ hg update fc627a69481f
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd ..
  $ hg update 11
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  11+
  $ hg -R s id
  fc627a69481f
  $ hg -R t id 
  e95bcfa18a35
