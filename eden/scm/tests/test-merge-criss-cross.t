#chg-compatible

  $ . helpers-usechg.sh

Criss cross merging

  $ hg init criss-cross
  $ cd criss-cross
  $ echo '0 base' > f1
  $ echo '0 base' > f2
  $ hg ci -Aqm '0 base'

  $ echo '1 first change' > f1
  $ hg ci -m '1 first change f1'

  $ hg up -qr0
  $ echo '2 first change' > f2
  $ mkdir d1
  $ echo '0 base' > d1/f3
  $ echo '0 base' > d1/f4
  $ hg add -q d1
  $ hg ci -qm '2 first change f2'

  $ hg merge -qr 1
  $ hg rm d1/f3
  $ hg mv -q d1 d2
  $ hg ci -m '3 merge'

  $ hg up -qr2
  $ hg merge -qr1
  $ hg ci -qm '4 merge'

  $ echo '5 second change' > f1
  $ hg ci -m '5 second change f1'

  $ hg up -r3
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  $ echo '6 second change' > f2
  $ hg ci -m '6 second change f2'

  $ hg log -G
  @  changeset:   6:6373bbfdae1d
  |  parent:      3:c202c8af058d
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     6 second change f2
  |
  | o  changeset:   5:e673248094b1
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     5 second change f1
  | |
  | o    changeset:   4:177f58377c06
  | |\   parent:      2:d1d156401c1b
  | | |  parent:      1:0f6b37dbe527
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     4 merge
  | | |
  o---+  changeset:   3:c202c8af058d
  | | |  parent:      2:d1d156401c1b
  |/ /   parent:      1:0f6b37dbe527
  | |    user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     3 merge
  | |
  | o  changeset:   2:d1d156401c1b
  | |  parent:      0:40494bf2444c
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     2 first change f2
  | |
  o |  changeset:   1:0f6b37dbe527
  |/   user:        test
  |    date:        Thu Jan 01 00:00:00 1970 +0000
  |    summary:     1 first change f1
  |
  o  changeset:   0:40494bf2444c
     user:        test
     date:        Thu Jan 01 00:00:00 1970 +0000
     summary:     0 base
  

  $ hg merge -v --debug --tool internal:dump 5 --config merge.preferancestor='!'
  note: using 0f6b37dbe527 as ancestor of 6373bbfdae1d and e673248094b1
        alternatively, use --config merge.preferancestor=d1d156401c1b
    searching for copies back to rev 3
    unmatched files in local:
     d2/f4
    unmatched files in other:
     d1/f3
     d1/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
     pending file src: 'd1/f3' -> dst: 'd2/f3'
     pending file src: 'd1/f4' -> dst: 'd2/f4'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 6373bbfdae1d+, remote: e673248094b1
   preserving d2/f4 for resolve of d2/f4
   preserving f2 for resolve of f2
   f1: remote is newer -> g
  getting f1
   d2/f3: local directory rename - get from d1/f3 -> dg
  getting d1/f3 to d2/f3
   d2/f4: local directory rename, both created -> m (premerge)
   f2: versions differ -> m (premerge)
  picked tool ':dump' for f2 (binary False symlink False changedelete False)
  merging f2
  my f2@6373bbfdae1d+ other f2@e673248094b1 ancestor f2@0f6b37dbe527
   f2: versions differ -> m (merge)
  picked tool ':dump' for f2 (binary False symlink False changedelete False)
  my f2@6373bbfdae1d+ other f2@e673248094b1 ancestor f2@0f6b37dbe527
  3 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ f --dump --recurse *
  d2: directory with 2 files
  d2/f3:
  >>>
  0 base
  <<<
  d2/f4:
  >>>
  0 base
  <<<
  f1:
  >>>
  5 second change
  <<<
  f2:
  >>>
  6 second change
  <<<
  f2.base:
  >>>
  0 base
  <<<
  f2.local:
  >>>
  6 second change
  <<<
  f2.orig:
  >>>
  6 second change
  <<<
  f2.other:
  >>>
  2 first change
  <<<

  $ hg up -qC .
  $ hg merge -v --tool internal:dump 5 --config merge.preferancestor="null 40663881 3b08d"
  note: using 0f6b37dbe527 as ancestor of 6373bbfdae1d and e673248094b1
        alternatively, use --config merge.preferancestor=d1d156401c1b
  resolving manifests
  getting f1
  getting d1/f3 to d2/f3
  merging f2
  3 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

Redo merge with merge.preferancestor="*" to enable bid merge

  $ rm f*
  $ hg up -qC .
  $ hg merge -v --debug --tool internal:dump 5 --config merge.preferancestor="*"
  note: merging 6373bbfdae1d+ and e673248094b1 using bids from ancestors 0f6b37dbe527 and d1d156401c1b
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
    unmatched files in local:
     d2/f4
    unmatched files in other:
     d1/f3
     d1/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
     pending file src: 'd1/f3' -> dst: 'd2/f3'
     pending file src: 'd1/f4' -> dst: 'd2/f4'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 6373bbfdae1d+, remote: e673248094b1
   d2/f3: local directory rename - get from d1/f3 -> dg
   d2/f4: local directory rename, both created -> m
   f1: remote is newer -> g
   f2: versions differ -> m
  
  calculating bids for ancestor d1d156401c1b
    searching for copies back to rev 3
    unmatched files in local:
     d2/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: d1d156401c1b, local: 6373bbfdae1d+, remote: e673248094b1
   f1: versions differ -> m
   f2: remote unchanged -> k
  
  auction for merging merge bids
   d2/f3: consensus for dg
   d2/f4: consensus for m
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
   preserving d2/f4 for resolve of d2/f4
   f1: remote is newer -> g
  getting f1
   f2: remote unchanged -> k
   d2/f3: local directory rename - get from d1/f3 -> dg
  getting d1/f3 to d2/f3
   d2/f4: local directory rename, both created -> m (premerge)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ f --dump --recurse *
  d2: directory with 2 files
  d2/f3:
  >>>
  0 base
  <<<
  d2/f4:
  >>>
  0 base
  <<<
  f1:
  >>>
  5 second change
  <<<
  f2:
  >>>
  6 second change
  <<<


The other way around:

  $ hg up -C -r5
  4 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ hg merge -v --debug --config merge.preferancestor="*"
  note: merging e673248094b1+ and 6373bbfdae1d using bids from ancestors 0f6b37dbe527 and d1d156401c1b
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
    unmatched files in local:
     d1/f3
     d1/f4
    unmatched files in other:
     d2/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
     pending file src: 'd1/f3' -> dst: 'd2/f3'
     pending file src: 'd1/f4' -> dst: 'd2/f4'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: e673248094b1+, remote: 6373bbfdae1d
   d2/f3: remote directory rename - move from d1/f3 -> dm
   d2/f4: remote directory rename, both created -> m
   f1: remote unchanged -> k
   f2: versions differ -> m
  
  calculating bids for ancestor d1d156401c1b
    searching for copies back to rev 3
    unmatched files in other:
     d2/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: d1d156401c1b, local: e673248094b1+, remote: 6373bbfdae1d
   d1/f3: other deleted -> r
   d1/f4: other deleted -> r
   d2/f4: remote created -> g
   f1: versions differ -> m
   f2: remote is newer -> g
  
  auction for merging merge bids
   d1/f3: consensus for r
   d1/f4: consensus for r
   d2/f3: consensus for dm
   d2/f4: picking 'get' action
   f1: picking 'keep' action
   f2: picking 'get' action
  end of auction
  
   d1/f3: other deleted -> r
  removing d1/f3
   d1/f4: other deleted -> r
  removing d1/f4
   d2/f4: remote created -> g
  getting d2/f4
   f2: remote is newer -> g
  getting f2
   f1: remote unchanged -> k
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ f --dump --recurse *
  d2: directory with 2 files
  d2/f3:
  >>>
  0 base
  <<<
  d2/f4:
  >>>
  0 base
  <<<
  f1:
  >>>
  5 second change
  <<<
  f2:
  >>>
  6 second change
  <<<

Verify how the output looks and how verbose it is:

  $ hg up -qC
  $ hg merge
  2 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg up -qC tip
  $ hg merge -v
  note: merging 6373bbfdae1d+ and e673248094b1 using bids from ancestors 0f6b37dbe527 and d1d156401c1b
  
  calculating bids for ancestor 0f6b37dbe527
  resolving manifests
  
  calculating bids for ancestor d1d156401c1b
  resolving manifests
  
  auction for merging merge bids
   d2/f3: consensus for dg
   d2/f4: consensus for m
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
  getting f1
  getting d1/f3 to d2/f3
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg up -qC
  $ hg merge -v --debug --config merge.preferancestor="*"
  note: merging 6373bbfdae1d+ and e673248094b1 using bids from ancestors 0f6b37dbe527 and d1d156401c1b
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
    unmatched files in local:
     d2/f4
    unmatched files in other:
     d1/f3
     d1/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
     pending file src: 'd1/f3' -> dst: 'd2/f3'
     pending file src: 'd1/f4' -> dst: 'd2/f4'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 6373bbfdae1d+, remote: e673248094b1
   d2/f3: local directory rename - get from d1/f3 -> dg
   d2/f4: local directory rename, both created -> m
   f1: remote is newer -> g
   f2: versions differ -> m
  
  calculating bids for ancestor d1d156401c1b
    searching for copies back to rev 3
    unmatched files in local:
     d2/f4
    all copies found (* = to merge, ! = divergent, % = renamed and deleted):
     src: 'd1/f4' -> dst: 'd2/f4' 
    checking for directory renames
     discovered dir src: 'd1/' -> dst: 'd2/'
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: d1d156401c1b, local: 6373bbfdae1d+, remote: e673248094b1
   f1: versions differ -> m
   f2: remote unchanged -> k
  
  auction for merging merge bids
   d2/f3: consensus for dg
   d2/f4: consensus for m
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
   preserving d2/f4 for resolve of d2/f4
   f1: remote is newer -> g
  getting f1
   f2: remote unchanged -> k
   d2/f3: local directory rename - get from d1/f3 -> dg
  getting d1/f3 to d2/f3
   d2/f4: local directory rename, both created -> m (premerge)
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ cd ..

http://stackoverflow.com/questions/9350005/how-do-i-specify-a-merge-base-to-use-in-a-hg-merge/9430810

  $ hg init ancestor-merging
  $ cd ancestor-merging
  $ echo a > x
  $ hg commit -A -m a x
  $ hg update -q 0
  $ echo b >> x
  $ hg commit -m b
  $ hg update -q 0
  $ echo c >> x
  $ hg commit -qm c
  $ hg update -q 1
  $ hg merge -q --tool internal:local 2
  $ echo c >> x
  $ hg commit -m bc
  $ hg update -q 2
  $ hg merge -q --tool internal:local 1
  $ echo b >> x
  $ hg commit -qm cb

  $ hg merge --config merge.preferancestor='!'
  note: using 70008a2163f6 as ancestor of 0d355fdef312 and 4b8b546a3eef
        alternatively, use --config merge.preferancestor=b211bbc6eb3c
  merging x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat x
  a
  c
  b
  c

  $ hg up -qC .

  $ hg merge --config merge.preferancestor=b211bbc6eb3c
  note: using b211bbc6eb3c as ancestor of 0d355fdef312 and 4b8b546a3eef
        alternatively, use --config merge.preferancestor=70008a2163f6
  merging x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat x
  a
  b
  c
  b

  $ hg up -qC .

  $ hg merge -v --config merge.preferancestor="*"
  note: merging 0d355fdef312+ and 4b8b546a3eef using bids from ancestors 70008a2163f6 and b211bbc6eb3c
  
  calculating bids for ancestor 70008a2163f6
  resolving manifests
  
  calculating bids for ancestor b211bbc6eb3c
  resolving manifests
  
  auction for merging merge bids
   x: multiple bids for merge action:
    versions differ -> m
    versions differ -> m
   x: ambiguous merge - picked m action
  end of auction
  
  merging x
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ cat x
  a
  c
  b
  c

merge.preferancestor does not affect revsets

  $ hg log -r 'ancestor(head())' --config merge.preferancestor=1 -T '{rev}\n'
  1
  $ hg log -r 'ancestor(head())' --config merge.preferancestor=2 -T '{rev}\n'
  1
  $ hg log -r 'ancestor(head())' --config merge.preferancestor=3 -T '{rev}\n'
  1
  $ hg log -r 'ancestor(head())' --config merge.preferancestor='1337 * - 2' -T '{rev}\n'
  1

  $ cd ..
