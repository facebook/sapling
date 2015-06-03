Let commit recurse into subrepos by default to match pre-2.0 behavior:

  $ echo "[ui]" >> $HGRCPATH
  $ echo "commitsubrepos = Yes" >> $HGRCPATH

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

  $ hg -R s add s/a
  $ hg files -S
  .hgsub
  a
  s/a (glob)

  $ hg -R s ci -Ams0
  $ hg sum
  parent: 0:f7b1eb17ad24 tip
   0
  branch: default
  commit: 1 added, 1 subrepos
  update: (current)
  phases: 1 draft
  $ hg ci -m1

test handling .hgsubstate "added" explicitly.

  $ hg parents --template '{node}\n{files}\n'
  7cf8cfea66e410e8e3336508dfeec07b3192de51
  .hgsub .hgsubstate
  $ hg rollback -q
  $ hg add .hgsubstate
  $ hg ci -m1
  $ hg parents --template '{node}\n{files}\n'
  7cf8cfea66e410e8e3336508dfeec07b3192de51
  .hgsub .hgsubstate

Revert subrepo and test subrepo fileset keyword:

  $ echo b > s/a
  $ hg revert --dry-run "set:subrepo('glob:s*')"
  reverting subrepo s
  reverting s/a (glob)
  $ cat s/a
  b
  $ hg revert "set:subrepo('glob:s*')"
  reverting subrepo s
  reverting s/a (glob)
  $ cat s/a
  a
  $ rm s/a.orig

Revert subrepo with no backup. The "reverting s/a" line is gone since
we're really running 'hg update' in the subrepo:

  $ echo b > s/a
  $ hg revert --no-backup s
  reverting subrepo s

Issue2022: update -C

  $ echo b > s/a
  $ hg sum
  parent: 1:7cf8cfea66e4 tip
   1
  branch: default
  commit: 1 subrepos
  update: (current)
  phases: 2 draft
  $ hg co -C 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg sum
  parent: 1:7cf8cfea66e4 tip
   1
  branch: default
  commit: (clean)
  update: (current)
  phases: 2 draft

commands that require a clean repo should respect subrepos

  $ echo b >> s/a
  $ hg backout tip
  abort: uncommitted changes in subrepository 's'
  [255]
  $ hg revert -C -R s s/a

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
  phases: 2 draft
  $ hg ci -m2
  committing subrepository s
  committing subrepository s/ss (glob)
  $ hg sum
  parent: 2:df30734270ae tip
   2
  branch: default
  commit: (clean)
  update: (current)
  phases: 3 draft

test handling .hgsubstate "modified" explicitly.

  $ hg parents --template '{node}\n{files}\n'
  df30734270ae757feb35e643b7018e818e78a9aa
  .hgsubstate
  $ hg rollback -q
  $ hg status -A .hgsubstate
  M .hgsubstate
  $ hg ci -m2
  $ hg parents --template '{node}\n{files}\n'
  df30734270ae757feb35e643b7018e818e78a9aa
  .hgsubstate

bump sub rev (and check it is ignored by ui.commitsubrepos)

  $ echo b > s/a
  $ hg -R s ci -ms1
  $ hg --config ui.commitsubrepos=no ci -m3

leave sub dirty (and check ui.commitsubrepos=no aborts the commit)

  $ echo c > s/a
  $ hg --config ui.commitsubrepos=no ci -m4
  abort: uncommitted changes in subrepository 's'
  (use --subrepos for recursive commit)
  [255]
  $ hg id
  f6affe3fbfaa+ tip
  $ hg -R s ci -mc
  $ hg id
  f6affe3fbfaa+ tip
  $ echo d > s/a
  $ hg ci -m4
  committing subrepository s
  $ hg tip -R s
  changeset:   4:02dcf1d70411
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
   revision 02dcf1d704118aee3ee306ccfa1910850d5b05ef

new branch for merge tests

  $ hg co 1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo t = t >> .hgsub
  $ hg init t
  $ echo t > t/t
  $ hg -R t add t
  adding t/t (glob)

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

test handling .hgsubstate "removed" explicitly.

  $ hg parents --template '{node}\n{files}\n'
  96615c1dad2dc8e3796d7332c77ce69156f7b78e
  .hgsub .hgsubstate
  $ hg rollback -q
  $ hg remove .hgsubstate
  $ hg ci -m8
  $ hg parents --template '{node}\n{files}\n'
  96615c1dad2dc8e3796d7332c77ce69156f7b78e
  .hgsub .hgsubstate

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
   branchmerge: True, force: False, partial: False
   ancestor: 1f14a2e2d3ec, local: f0d2028bf86d+, remote: 1831e14459c4
   .hgsubstate: versions differ -> m
  subrepo merge f0d2028bf86d+ 1831e14459c4 1f14a2e2d3ec
    subrepo t: other changed, get t:6747d179aa9a688023c4b0cad32e4c92bb7f34ad:hg
  getting subrepo t
  resolving manifests
   branchmerge: False, force: False, partial: False
   ancestor: 60ca1237c194, local: 60ca1237c194+, remote: 6747d179aa9a
   t: remote is newer -> g
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
   branchmerge: True, force: False, partial: False
   ancestor: 1831e14459c4, local: e45c8b14af55+, remote: f94576341bcf
   .hgsubstate: versions differ -> m
  subrepo merge e45c8b14af55+ f94576341bcf 1831e14459c4
    subrepo t: both sides changed 
   subrepository t diverged (local revision: 20a0db6fbf6c, remote revision: 7af322bc1198)
  (M)erge, keep (l)ocal or keep (r)emote? m
  merging subrepo t
    searching for copies back to rev 2
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 6747d179aa9a, local: 20a0db6fbf6c+, remote: 7af322bc1198
   preserving t for resolve of t
   t: versions differ -> m
  picked tool 'internal:merge' for t (binary False symlink False)
  merging t
  my t@20a0db6fbf6c+ other t@7af322bc1198 ancestor t@6747d179aa9a
  warning: conflicts during merge.
  merging t incomplete! (edit conflicts, then use 'hg resolve --mark')
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
    subrepo t: merge with t:7af322bc1198a32402fe903e0b7ebcfc5c9bf8f4:hg
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

should conflict

  $ cat t/t
  <<<<<<< local: 20a0db6fbf6c - test: 10
  conflict
  =======
  t3
  >>>>>>> other: 7af322bc1198  - test: 7

11: remove subrepo t

  $ hg co -C 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg revert -r 4 .hgsub # remove t
  $ hg ci -m11
  created new head
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4

local removed, remote changed, keep changed

  $ hg merge 6
   remote changed subrepository t which local removed
  use (c)hanged version or (d)elete? c
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
BROKEN: should include subrepo t
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  6747d179aa9a688023c4b0cad32e4c92bb7f34ad t
  $ hg ci -m 'local removed, remote changed, keep changed'
BROKEN: should include subrepo t
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
BROKEN: should include subrepo t
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ cat t/t
  t2

local removed, remote changed, keep removed

  $ hg co -C 11
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge --config ui.interactive=true 6 <<EOF
  > d
  > EOF
   remote changed subrepository t which local removed
  use (c)hanged version or (d)elete? d
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ hg ci -m 'local removed, remote changed, keep removed'
  created new head
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s

local changed, remote removed, keep changed

  $ hg co -C 6
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 11
   local changed subrepository t which remote removed
  use (c)hanged version or (d)elete? c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
BROKEN: should include subrepo t
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
BROKEN: should include subrepo t
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ hg ci -m 'local changed, remote removed, keep changed'
  created new head
BROKEN: should include subrepo t
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
BROKEN: should include subrepo t
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ cat t/t
  t2

local changed, remote removed, keep removed

  $ hg co -C 6
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge --config ui.interactive=true 11 <<EOF
  > d
  > EOF
   local changed subrepository t which remote removed
  use (c)hanged version or (d)elete? d
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ hg ci -m 'local changed, remote removed, keep removed'
  created new head
  $ hg debugsub
  path s
   source   s
   revision e4ece1bf43360ddc8f6a96432201a37b7cd27ae4
  $ cat .hgsubstate
  e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s

clean up to avoid having to fix up the tests below

  $ hg co -C 10
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > strip=
  > EOF
  $ hg strip -r 11:15
  saved backup bundle to $TESTTMP/t/.hg/strip-backup/*-backup.hg (glob)

clone

  $ cd ..
  $ hg clone t tc
  updating to branch default
  cloning subrepo s from $TESTTMP/t/s
  cloning subrepo s/ss from $TESTTMP/t/s/ss (glob)
  cloning subrepo t from $TESTTMP/t/t
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
  pushing to $TESTTMP/t (glob)
  no changes made to subrepo s/ss since last push to $TESTTMP/t/s/ss (glob)
  no changes made to subrepo s since last push to $TESTTMP/t/s
  pushing subrepo t to $TESTTMP/t/t
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
  pushing to $TESTTMP/t (glob)
  no changes made to subrepo s/ss since last push to $TESTTMP/t/s/ss (glob)
  pushing subrepo s to $TESTTMP/t/s
  searching for changes
  abort: push creates new remote head 12a213df6fa9! (in subrepo s)
  (merge or see "hg help push" for details about pushing new heads)
  [255]
  $ hg push -f
  pushing to $TESTTMP/t (glob)
  pushing subrepo s/ss to $TESTTMP/t/s/ss (glob)
  searching for changes
  no changes found
  pushing subrepo s to $TESTTMP/t/s
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  pushing subrepo t to $TESTTMP/t/t
  searching for changes
  no changes found
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

check that unmodified subrepos are not pushed

  $ hg clone . ../tcc
  updating to branch default
  cloning subrepo s from $TESTTMP/tc/s
  cloning subrepo s/ss from $TESTTMP/tc/s/ss (glob)
  cloning subrepo t from $TESTTMP/tc/t
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

the subrepos on the new clone have nothing to push to its source

  $ hg push -R ../tcc .
  pushing to .
  no changes made to subrepo s/ss since last push to s/ss (glob)
  no changes made to subrepo s since last push to s
  no changes made to subrepo t since last push to t
  searching for changes
  no changes found
  [1]

the subrepos on the source do not have a clean store versus the clone target
because they were never explicitly pushed to the source

  $ hg push ../tcc
  pushing to ../tcc
  pushing subrepo s/ss to ../tcc/s/ss (glob)
  searching for changes
  no changes found
  pushing subrepo s to ../tcc/s
  searching for changes
  no changes found
  pushing subrepo t to ../tcc/t
  searching for changes
  no changes found
  searching for changes
  no changes found
  [1]

after push their stores become clean

  $ hg push ../tcc
  pushing to ../tcc
  no changes made to subrepo s/ss since last push to ../tcc/s/ss (glob)
  no changes made to subrepo s since last push to ../tcc/s
  no changes made to subrepo t since last push to ../tcc/t
  searching for changes
  no changes found
  [1]

updating a subrepo to a different revision or changing
its working directory does not make its store dirty

  $ hg -R s update '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg push
  pushing to $TESTTMP/t (glob)
  no changes made to subrepo s/ss since last push to $TESTTMP/t/s/ss (glob)
  no changes made to subrepo s since last push to $TESTTMP/t/s
  no changes made to subrepo t since last push to $TESTTMP/t/t
  searching for changes
  no changes found
  [1]
  $ echo foo >> s/a
  $ hg push
  pushing to $TESTTMP/t (glob)
  no changes made to subrepo s/ss since last push to $TESTTMP/t/s/ss (glob)
  no changes made to subrepo s since last push to $TESTTMP/t/s
  no changes made to subrepo t since last push to $TESTTMP/t/t
  searching for changes
  no changes found
  [1]
  $ hg -R s update -C tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

committing into a subrepo makes its store (but not its parent's store) dirty

  $ echo foo >> s/ss/a
  $ hg -R s/ss commit -m 'test dirty store detection'

  $ hg out -S -r `hg log -r tip -T "{node|short}"`
  comparing with $TESTTMP/t (glob)
  searching for changes
  no changes found
  comparing with $TESTTMP/t/s
  searching for changes
  no changes found
  comparing with $TESTTMP/t/s/ss
  searching for changes
  changeset:   1:79ea5566a333
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     test dirty store detection
  
  comparing with $TESTTMP/t/t
  searching for changes
  no changes found

  $ hg push
  pushing to $TESTTMP/t (glob)
  pushing subrepo s/ss to $TESTTMP/t/s/ss (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  no changes made to subrepo s since last push to $TESTTMP/t/s
  no changes made to subrepo t since last push to $TESTTMP/t/t
  searching for changes
  no changes found
  [1]

a subrepo store may be clean versus one repo but not versus another

  $ hg push
  pushing to $TESTTMP/t (glob)
  no changes made to subrepo s/ss since last push to $TESTTMP/t/s/ss (glob)
  no changes made to subrepo s since last push to $TESTTMP/t/s
  no changes made to subrepo t since last push to $TESTTMP/t/t
  searching for changes
  no changes found
  [1]
  $ hg push ../tcc
  pushing to ../tcc
  pushing subrepo s/ss to ../tcc/s/ss (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  no changes made to subrepo s since last push to ../tcc/s
  no changes made to subrepo t since last push to ../tcc/t
  searching for changes
  no changes found
  [1]

update

  $ cd ../t
  $ hg up -C # discard our earlier merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo blah > t/t
  $ hg ci -m13
  committing subrepository t

backout calls revert internally with minimal opts, which should not raise
KeyError

  $ hg backout ".^"
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  changeset c373c8102e68 backed out, don't forget to commit.

  $ hg up -C # discard changes
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

pull

  $ cd ../tc
  $ hg pull
  pulling from $TESTTMP/t (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  (run 'hg update' to get a working copy)

should pull t

  $ hg incoming -S -r `hg log -r tip -T "{node|short}"`
  comparing with $TESTTMP/t (glob)
  no changes found
  comparing with $TESTTMP/t/s
  searching for changes
  no changes found
  comparing with $TESTTMP/t/s/ss
  searching for changes
  no changes found
  comparing with $TESTTMP/t/t
  searching for changes
  changeset:   5:52c0adc0515a
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     13
  

  $ hg up
  pulling subrepo t from $TESTTMP/t/t
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
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
  (branches are permanent and global, did you want a bookmark?)
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
  $ hg branch br
  marked working directory as branch br
  (branches are permanent and global, did you want a bookmark?)
  $ echo b > b
  $ hg -R s up 3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg ci -Am1
  adding b
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
  $ hg up 3
  2 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg -R s up 5
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo e > e
  $ hg ci -Am1
  adding e

  $ hg up 5
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge 4    # try to merge default into br again
   subrepository s diverged (local revision: f8f13b33206e, remote revision: a3f9062a4f88)
  (M)erge, keep (l)ocal or keep (r)emote? m
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
  adding testdelete/nested/foo (glob)
  $ hg -R testdelete/nested2 add
  adding testdelete/nested2/foo (glob)
  $ hg -R testdelete/nested ci -m test
  $ hg -R testdelete/nested2 ci -m test
  $ echo nested = nested > testdelete/.hgsub
  $ echo nested2 = nested2 >> testdelete/.hgsub
  $ hg -R testdelete add
  adding testdelete/.hgsub (glob)
  $ hg -R testdelete ci -m "nested 1 & 2 added"
  $ echo nested = nested > testdelete/.hgsub
  $ hg -R testdelete ci -m "nested 2 deleted"
  $ cat testdelete/.hgsubstate
  bdf5c9a3103743d900b12ae0db3ffdcfd7b0d878 nested
  $ hg -R testdelete remove testdelete/.hgsub
  $ hg -R testdelete ci -m ".hgsub deleted"
  $ cat testdelete/.hgsubstate
  bdf5c9a3103743d900b12ae0db3ffdcfd7b0d878 nested

test repository cloning

  $ mkdir mercurial mercurial2
  $ hg init nested_absolute
  $ echo test > nested_absolute/foo
  $ hg -R nested_absolute add
  adding nested_absolute/foo (glob)
  $ hg -R nested_absolute ci -mtest
  $ cd mercurial
  $ hg init nested_relative
  $ echo test2 > nested_relative/foo2
  $ hg -R nested_relative add
  adding nested_relative/foo2 (glob)
  $ hg -R nested_relative ci -mtest2
  $ hg init main
  $ echo "nested_relative = ../nested_relative" > main/.hgsub
  $ echo "nested_absolute = `pwd`/nested_absolute" >> main/.hgsub
  $ hg -R main add
  adding main/.hgsub (glob)
  $ hg -R main ci -m "add subrepos"
  $ cd ..
  $ hg clone mercurial/main mercurial2/main
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat mercurial2/main/nested_absolute/.hg/hgrc \
  >     mercurial2/main/nested_relative/.hg/hgrc
  [paths]
  default = $TESTTMP/mercurial/nested_absolute
  [paths]
  default = $TESTTMP/mercurial/nested_relative
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
  $ hg clone repo repo2
  updating to branch default
  cloning subrepo s from $TESTTMP/repo/s
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -q -R repo2 pull -u
  $ echo 1 > repo2/s/a
  $ hg -R repo2/s ci -m2
  $ hg -q -R repo2/s push
  $ hg -R repo2/s up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo 2 > repo2/s/b
  $ hg -R repo2/s ci -m3 -A
  adding b
  created new head
  $ hg -R repo2 ci -m3
  $ hg -q -R repo2 push
  abort: push creates new remote head cc505f09a8b2! (in subrepo s)
  (merge or see "hg help push" for details about pushing new heads)
  [255]
  $ hg -R repo update
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved

test if untracked file is not overwritten

  $ echo issue3276_ok > repo/s/b
  $ hg -R repo2 push -f -q
  $ touch -t 200001010000 repo/.hgsubstate
  $ hg -R repo status --config debug.dirstate.delaywrite=2 repo/.hgsubstate
  $ hg -R repo update
  b: untracked file differs
  abort: untracked files in working directory differ from files in requested revision (in subrepo s)
  [255]

  $ cat repo/s/b
  issue3276_ok
  $ rm repo/s/b
  $ touch -t 200001010000 repo/.hgsubstate
  $ hg -R repo revert --all
  reverting repo/.hgsubstate (glob)
  reverting subrepo s
  $ hg -R repo update
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat repo/s/b
  2
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
  committing subrepository sub/repo (glob)
  $ echo test >> sub/repo/foo
  $ hg ci -mtest
  committing subrepository sub/repo (glob)
  $ hg cat sub/repo/foo
  test
  test
  $ mkdir -p tmp/sub/repo
  $ hg cat -r 0 --output tmp/%p_p sub/repo/foo
  $ cat tmp/sub/repo/foo_p
  test
  $ mv sub/repo sub_
  $ hg cat sub/repo/baz
  skipping missing subrepository: sub/repo
  [1]
  $ rm -rf sub/repo
  $ mv sub_ sub/repo
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
  abort: default path for subrepository not found (in subrepo sub/repo) (glob)
  [255]

Ensure a full traceback, not just the SubrepoAbort part

  $ hg -R issue1852b update --traceback 2>&1 | grep 'raise util\.Abort'
      raise util.Abort(_("default path for subrepository not found"))

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
  cloning subrepo sub/repo from issue1852a/sub/repo (glob)
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

Try to push from the other side

  $ hg -R issue1852a push `pwd`/issue1852c
  pushing to $TESTTMP/issue1852c (glob)
  pushing subrepo sub/repo to $TESTTMP/issue1852c/sub/repo (glob)
  searching for changes
  no changes found
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

Incoming and outgoing should not use the default path:

  $ hg clone -q issue1852a issue1852d
  $ hg -R issue1852d outgoing --subrepos issue1852c
  comparing with issue1852c
  searching for changes
  no changes found
  comparing with issue1852c/sub/repo
  searching for changes
  no changes found
  [1]
  $ hg -R issue1852d incoming --subrepos issue1852c
  comparing with issue1852c
  searching for changes
  no changes found
  comparing with issue1852c/sub/repo
  searching for changes
  no changes found
  [1]

Check that merge of a new subrepo doesn't write the uncommitted state to
.hgsubstate (issue4622)

  $ hg init issue1852a/addedsub
  $ echo zzz > issue1852a/addedsub/zz.txt
  $ hg -R issue1852a/addedsub ci -Aqm "initial ZZ"

  $ hg clone issue1852a/addedsub issue1852d/addedsub
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

  $ echo def > issue1852a/sub/repo/foo
  $ hg -R issue1852a ci -SAm 'tweaked subrepo'
  adding tmp/sub/repo/foo_p
  committing subrepository sub/repo (glob)

  $ echo 'addedsub = addedsub' >> issue1852d/.hgsub
  $ echo xyz > issue1852d/sub/repo/foo
  $ hg -R issue1852d pull -u
  pulling from $TESTTMP/issue1852a (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 2 changes to 2 files
   subrepository sub/repo diverged (local revision: f42d5c7504a8, remote revision: 46cd4aac504c)
  (M)erge, keep (l)ocal or keep (r)emote? m
  pulling subrepo sub/repo from $TESTTMP/issue1852a/sub/repo (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
   subrepository sources for sub/repo differ (glob)
  use (l)ocal source (f42d5c7504a8) or (r)emote source (46cd4aac504c)? l
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cat issue1852d/.hgsubstate
  f42d5c7504a811dda50f5cf3e5e16c3330b87172 sub/repo

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
  committing subrepository subrepo-2
  $ hg st subrepo-2/file

Check that share works with subrepo
  $ hg --config extensions.share= share . ../shared
  updating working directory
  cloning subrepo subrepo-2 from $TESTTMP/subrepo-status/subrepo-2
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ test -f ../shared/subrepo-1/.hg/sharedpath
  [1]
  $ hg -R ../shared in
  abort: repository default not found!
  [255]
  $ hg -R ../shared/subrepo-2 showconfig paths
  paths.default=$TESTTMP/subrepo-status/subrepo-2
  $ hg -R ../shared/subrepo-1 sum --remote
  parent: -1:000000000000 tip (empty repository)
  branch: default
  commit: (clean)
  update: (current)
  remote: (synced)

Check hg update --clean
  $ cd $TESTTMP/t
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
  $ cd $TESTTMP/t
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

Sticky subrepositories, file changes
  $ touch s/f1
  $ touch t/f1
  $ hg add -S s/f1
  $ hg add -S t/f1
  $ hg id
  365661e5936a+
  $ hg -R s id
  fc627a69481f+
  $ hg -R t id
  e95bcfa18a35+
  $ hg update tip
   subrepository s diverged (local revision: fc627a69481f, remote revision: 12a213df6fa9)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ
  use (l)ocal source (fc627a69481f) or (r)emote source (12a213df6fa9)? l
   subrepository t diverged (local revision: e95bcfa18a35, remote revision: 52c0adc0515a)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for t differ
  use (l)ocal source (e95bcfa18a35) or (r)emote source (52c0adc0515a)? l
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
   subrepository s diverged (local revision: 12a213df6fa9, remote revision: fc627a69481f)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository t diverged (local revision: 52c0adc0515a, remote revision: 20a0db6fbf6c)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for t differ (in checked out version)
  use (l)ocal source (7af322bc1198) or (r)emote source (20a0db6fbf6c)? l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  e45c8b14af55+
  $ hg -R s id
  02dcf1d70411
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
  02dcf1d70411+
  $ hg -R t id
  7af322bc1198+
  $ hg update tip
   subrepository s diverged (local revision: 12a213df6fa9, remote revision: 12a213df6fa9)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for s differ
  use (l)ocal source (02dcf1d70411) or (r)emote source (12a213df6fa9)? l
   subrepository t diverged (local revision: 52c0adc0515a, remote revision: 52c0adc0515a)
  (M)erge, keep (l)ocal or keep (r)emote? m
   subrepository sources for t differ
  use (l)ocal source (7af322bc1198) or (r)emote source (52c0adc0515a)? l
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id
  925c17564ef8+ tip
  $ hg -R s id
  02dcf1d70411+
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
   subrepository s diverged (local revision: 12a213df6fa9, remote revision: fc627a69481f)
  (M)erge, keep (l)ocal or keep (r)emote? m
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg id -n
  11+
  $ hg -R s id
  fc627a69481f
  $ hg -R t id
  e95bcfa18a35

Test that removing .hgsubstate doesn't break anything:

  $ hg rm -f .hgsubstate
  $ hg ci -mrm
  nothing changed
  [1]
  $ hg log -vr tip
  changeset:   13:925c17564ef8
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       .hgsubstate
  description:
  13
  
  

Test that removing .hgsub removes .hgsubstate:

  $ hg rm .hgsub
  $ hg ci -mrm2
  created new head
  $ hg log -vr tip
  changeset:   14:2400bccd50af
  tag:         tip
  parent:      11:365661e5936a
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       .hgsub .hgsubstate
  description:
  rm2
  
  
Test issue3153: diff -S with deleted subrepos

  $ hg diff --nodates -S -c .
  diff -r 365661e5936a -r 2400bccd50af .hgsub
  --- a/.hgsub
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -s = s
  -t = t
  diff -r 365661e5936a -r 2400bccd50af .hgsubstate
  --- a/.hgsubstate
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -fc627a69481fcbe5f1135069e8a3881c023e4cf5 s
  -e95bcfa18a358dc4936da981ebf4147b4cad1362 t

Test behavior of add for explicit path in subrepo:
  $ cd ..
  $ hg init explicit
  $ cd explicit
  $ echo s = s > .hgsub
  $ hg add .hgsub
  $ hg init s
  $ hg ci -m0
Adding with an explicit path in a subrepo adds the file
  $ echo c1 > f1
  $ echo c2 > s/f2
  $ hg st -S
  ? f1
  ? s/f2
  $ hg add s/f2
  $ hg st -S
  A s/f2
  ? f1
  $ hg ci -R s -m0
  $ hg ci -Am1
  adding f1
Adding with an explicit path in a subrepo with -S has the same behavior
  $ echo c3 > f3
  $ echo c4 > s/f4
  $ hg st -S
  ? f3
  ? s/f4
  $ hg add -S s/f4
  $ hg st -S
  A s/f4
  ? f3
  $ hg ci -R s -m1
  $ hg ci -Ama2
  adding f3
Adding without a path or pattern silently ignores subrepos
  $ echo c5 > f5
  $ echo c6 > s/f6
  $ echo c7 > s/f7
  $ hg st -S
  ? f5
  ? s/f6
  ? s/f7
  $ hg add
  adding f5
  $ hg st -S
  A f5
  ? s/f6
  ? s/f7
  $ hg ci -R s -Am2
  adding f6
  adding f7
  $ hg ci -m3
Adding without a path or pattern with -S also adds files in subrepos
  $ echo c8 > f8
  $ echo c9 > s/f9
  $ echo c10 > s/f10
  $ hg st -S
  ? f8
  ? s/f10
  ? s/f9
  $ hg add -S
  adding f8
  adding s/f10 (glob)
  adding s/f9 (glob)
  $ hg st -S
  A f8
  A s/f10
  A s/f9
  $ hg ci -R s -m3
  $ hg ci -m4
Adding with a pattern silently ignores subrepos
  $ echo c11 > fm11
  $ echo c12 > fn12
  $ echo c13 > s/fm13
  $ echo c14 > s/fn14
  $ hg st -S
  ? fm11
  ? fn12
  ? s/fm13
  ? s/fn14
  $ hg add 'glob:**fm*'
  adding fm11
  $ hg st -S
  A fm11
  ? fn12
  ? s/fm13
  ? s/fn14
  $ hg ci -R s -Am4
  adding fm13
  adding fn14
  $ hg ci -Am5
  adding fn12
Adding with a pattern with -S also adds matches in subrepos
  $ echo c15 > fm15
  $ echo c16 > fn16
  $ echo c17 > s/fm17
  $ echo c18 > s/fn18
  $ hg st -S
  ? fm15
  ? fn16
  ? s/fm17
  ? s/fn18
  $ hg add -S 'glob:**fm*'
  adding fm15
  adding s/fm17 (glob)
  $ hg st -S
  A fm15
  A s/fm17
  ? fn16
  ? s/fn18
  $ hg ci -R s -Am5
  adding fn18
  $ hg ci -Am6
  adding fn16

Test behavior of forget for explicit path in subrepo:
Forgetting an explicit path in a subrepo untracks the file
  $ echo c19 > s/f19
  $ hg add s/f19
  $ hg st -S
  A s/f19
  $ hg forget s/f19
  $ hg st -S
  ? s/f19
  $ rm s/f19
  $ cd ..

Courtesy phases synchronisation to publishing server does not block the push
(issue3781)

  $ cp -r main issue3781
  $ cp -r main issue3781-dest
  $ cd issue3781-dest/s
  $ hg phase tip # show we have draft changeset
  5: draft
  $ chmod a-w .hg/store/phaseroots # prevent phase push
  $ cd ../../issue3781
  $ cat >> .hg/hgrc << EOF
  > [paths]
  > default=../issue3781-dest/
  > EOF
  $ hg push --config experimental.bundle2-exp=False
  pushing to $TESTTMP/issue3781-dest (glob)
  pushing subrepo s to $TESTTMP/issue3781-dest/s
  searching for changes
  no changes found
  searching for changes
  no changes found
  [1]
# clean the push cache
  $ rm s/.hg/cache/storehash/*
  $ hg push --config experimental.bundle2-exp=True
  pushing to $TESTTMP/issue3781-dest (glob)
  pushing subrepo s to $TESTTMP/issue3781-dest/s
  searching for changes
  no changes found
  searching for changes
  no changes found
  [1]
  $ cd ..

Test phase choice for newly created commit with "phases.subrepochecks"
configuration

  $ cd t
  $ hg update -q -r 12

  $ cat >> s/ss/.hg/hgrc <<EOF
  > [phases]
  > new-commit = secret
  > EOF
  $ cat >> s/.hg/hgrc <<EOF
  > [phases]
  > new-commit = draft
  > EOF
  $ echo phasecheck1 >> s/ss/a
  $ hg -R s commit -S --config phases.checksubrepos=abort -m phasecheck1
  committing subrepository ss
  transaction abort!
  rollback completed
  abort: can't commit in draft phase conflicting secret from subrepository ss
  [255]
  $ echo phasecheck2 >> s/ss/a
  $ hg -R s commit -S --config phases.checksubrepos=ignore -m phasecheck2
  committing subrepository ss
  $ hg -R s/ss phase tip
  3: secret
  $ hg -R s phase tip
  6: draft
  $ echo phasecheck3 >> s/ss/a
  $ hg -R s commit -S -m phasecheck3
  committing subrepository ss
  warning: changes are committed in secret phase from subrepository ss
  $ hg -R s/ss phase tip
  4: secret
  $ hg -R s phase tip
  7: secret

  $ cat >> t/.hg/hgrc <<EOF
  > [phases]
  > new-commit = draft
  > EOF
  $ cat >> .hg/hgrc <<EOF
  > [phases]
  > new-commit = public
  > EOF
  $ echo phasecheck4 >>   s/ss/a
  $ echo phasecheck4 >>   t/t
  $ hg commit -S -m phasecheck4
  committing subrepository s
  committing subrepository s/ss (glob)
  warning: changes are committed in secret phase from subrepository ss
  committing subrepository t
  warning: changes are committed in secret phase from subrepository s
  created new head
  $ hg -R s/ss phase tip
  5: secret
  $ hg -R s phase tip
  8: secret
  $ hg -R t phase tip
  6: draft
  $ hg phase tip
  15: secret

  $ cd ..


Test that commit --secret works on both repo and subrepo (issue4182)

  $ cd main
  $ echo secret >> b
  $ echo secret >> s/b
  $ hg commit --secret --subrepo -m "secret"
  committing subrepository s
  $ hg phase -r .
  6: secret
  $ cd s
  $ hg phase -r .
  6: secret
  $ cd ../../

Test "subrepos" template keyword

  $ cd t
  $ hg update -q 15
  $ cat > .hgsub <<EOF
  > s = s
  > EOF
  $ hg commit -m "16"
  warning: changes are committed in secret phase from subrepository s

(addition of ".hgsub" itself)

  $ hg diff --nodates -c 1 .hgsubstate
  diff -r f7b1eb17ad24 -r 7cf8cfea66e4 .hgsubstate
  --- /dev/null
  +++ b/.hgsubstate
  @@ -0,0 +1,1 @@
  +e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  $ hg log -r 1 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  f7b1eb17ad24 000000000000
  s

(modification of existing entry)

  $ hg diff --nodates -c 2 .hgsubstate
  diff -r 7cf8cfea66e4 -r df30734270ae .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,1 @@
  -e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  +dc73e2e6d2675eb2e41e33c205f4bdab4ea5111d s
  $ hg log -r 2 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  7cf8cfea66e4 000000000000
  s

(addition of entry)

  $ hg diff --nodates -c 5 .hgsubstate
  diff -r 7cf8cfea66e4 -r 1f14a2e2d3ec .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,2 @@
   e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  +60ca1237c19474e7a3978b0dc1ca4e6f36d51382 t
  $ hg log -r 5 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  7cf8cfea66e4 000000000000
  t

(removal of existing entry)

  $ hg diff --nodates -c 16 .hgsubstate
  diff -r 8bec38d2bd0b -r f2f70bc3d3c9 .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,2 +1,1 @@
   0731af8ca9423976d3743119d0865097c07bdc1b s
  -e202dc79b04c88a636ea8913d9182a1346d9b3dc t
  $ hg log -r 16 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  8bec38d2bd0b 000000000000
  t

(merging)

  $ hg diff --nodates -c 9 .hgsubstate
  diff -r f6affe3fbfaa -r f0d2028bf86d .hgsubstate
  --- a/.hgsubstate
  +++ b/.hgsubstate
  @@ -1,1 +1,2 @@
   fc627a69481fcbe5f1135069e8a3881c023e4cf5 s
  +60ca1237c19474e7a3978b0dc1ca4e6f36d51382 t
  $ hg log -r 9 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  f6affe3fbfaa 1f14a2e2d3ec
  t

(removal of ".hgsub" itself)

  $ hg diff --nodates -c 8 .hgsubstate
  diff -r f94576341bcf -r 96615c1dad2d .hgsubstate
  --- a/.hgsubstate
  +++ /dev/null
  @@ -1,2 +0,0 @@
  -e4ece1bf43360ddc8f6a96432201a37b7cd27ae4 s
  -7af322bc1198a32402fe903e0b7ebcfc5c9bf8f4 t
  $ hg log -r 8 --template "{p1node|short} {p2node|short}\n{subrepos % '{subrepo}\n'}"
  f94576341bcf 000000000000

Test that '[paths]' is configured correctly at subrepo creation

  $ cd $TESTTMP/tc
  $ cat > .hgsub <<EOF
  > # to clear bogus subrepo path 'bogus=[boguspath'
  > s = s
  > t = t
  > EOF
  $ hg update -q --clean null
  $ rm -rf s t
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default-push = /foo/bar
  > EOF
  $ hg update -q
  $ cat s/.hg/hgrc
  [paths]
  default = $TESTTMP/t/s
  default-push = /foo/bar/s
  $ cat s/ss/.hg/hgrc
  [paths]
  default = $TESTTMP/t/s/ss
  default-push = /foo/bar/s/ss
  $ cat t/.hg/hgrc
  [paths]
  default = $TESTTMP/t/t
  default-push = /foo/bar/t

  $ cd $TESTTMP/t
  $ hg up -qC 0
  $ echo 'bar' > bar.txt
  $ hg ci -Am 'branch before subrepo add'
  adding bar.txt
  created new head
  $ hg merge -r "first(subrepo('s'))"
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg status -S -X '.hgsub*'
  A s/a
  ? s/b
  ? s/c
  ? s/f1
  $ hg status -S --rev 'p2()'
  A bar.txt
  ? s/b
  ? s/c
  ? s/f1
  $ hg diff -S -X '.hgsub*' --nodates
  diff -r 000000000000 s/a
  --- /dev/null
  +++ b/s/a
  @@ -0,0 +1,1 @@
  +a
  $ hg diff -S --rev 'p2()' --nodates
  diff -r 7cf8cfea66e4 bar.txt
  --- /dev/null
  +++ b/bar.txt
  @@ -0,0 +1,1 @@
  +bar

  $ cd ..
