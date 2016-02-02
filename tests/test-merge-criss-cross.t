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
  $ hg ci -qm '2 first change f2'

  $ hg merge -qr 1
  $ hg ci -m '3 merge'

  $ hg up -qr2
  $ hg merge -qr1
  $ hg ci -qm '4 merge'

  $ echo '5 second change' > f1
  $ hg ci -m '5 second change f1'

  $ hg up -r3
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo '6 second change' > f2
  $ hg ci -m '6 second change f2'

  $ hg log -G
  @  changeset:   6:3b08d01b0ab5
  |  tag:         tip
  |  parent:      3:cf89f02107e5
  |  user:        test
  |  date:        Thu Jan 01 00:00:00 1970 +0000
  |  summary:     6 second change f2
  |
  | o  changeset:   5:adfe50279922
  | |  user:        test
  | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | |  summary:     5 second change f1
  | |
  | o    changeset:   4:7d3e55501ae6
  | |\   parent:      2:40663881a6dd
  | | |  parent:      1:0f6b37dbe527
  | | |  user:        test
  | | |  date:        Thu Jan 01 00:00:00 1970 +0000
  | | |  summary:     4 merge
  | | |
  o---+  changeset:   3:cf89f02107e5
  | | |  parent:      2:40663881a6dd
  |/ /   parent:      1:0f6b37dbe527
  | |    user:        test
  | |    date:        Thu Jan 01 00:00:00 1970 +0000
  | |    summary:     3 merge
  | |
  | o  changeset:   2:40663881a6dd
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
  note: using 0f6b37dbe527 as ancestor of 3b08d01b0ab5 and adfe50279922
        alternatively, use --config merge.preferancestor=40663881a6dd
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 3b08d01b0ab5+, remote: adfe50279922
   preserving f2 for resolve of f2
   f1: remote is newer -> g
  getting f1
   f2: versions differ -> m (premerge)
  picked tool ':dump' for f2 (binary False symlink False changedelete False)
  merging f2
  my f2@3b08d01b0ab5+ other f2@adfe50279922 ancestor f2@0f6b37dbe527
   f2: versions differ -> m (merge)
  picked tool ':dump' for f2 (binary False symlink False changedelete False)
  my f2@3b08d01b0ab5+ other f2@adfe50279922 ancestor f2@0f6b37dbe527
  1 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

  $ head *
  ==> f1 <==
  5 second change
  
  ==> f2 <==
  6 second change
  
  ==> f2.base <==
  0 base
  
  ==> f2.local <==
  6 second change
  
  ==> f2.orig <==
  6 second change
  
  ==> f2.other <==
  2 first change

  $ hg up -qC .
  $ hg merge -v --tool internal:dump 5 --config merge.preferancestor="null 40663881 3b08d"
  note: using 40663881a6dd as ancestor of 3b08d01b0ab5 and adfe50279922
        alternatively, use --config merge.preferancestor=0f6b37dbe527
  resolving manifests
  merging f1
  0 files updated, 0 files merged, 0 files removed, 1 files unresolved
  use 'hg resolve' to retry unresolved file merges or 'hg update -C .' to abandon
  [1]

Redo merge with merge.preferancestor="*" to enable bid merge

  $ rm f*
  $ hg up -qC .
  $ hg merge -v --debug --tool internal:dump 5 --config merge.preferancestor="*"
  note: merging 3b08d01b0ab5+ and adfe50279922 using bids from ancestors 0f6b37dbe527 and 40663881a6dd
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 3b08d01b0ab5+, remote: adfe50279922
   f1: remote is newer -> g
   f2: versions differ -> m
  
  calculating bids for ancestor 40663881a6dd
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 40663881a6dd, local: 3b08d01b0ab5+, remote: adfe50279922
   f1: versions differ -> m
   f2: remote unchanged -> k
  
  auction for merging merge bids
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
   f1: remote is newer -> g
  getting f1
   f2: remote unchanged -> k
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ head *
  ==> f1 <==
  5 second change
  
  ==> f2 <==
  6 second change


The other way around:

  $ hg up -C -r5
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg merge -v --debug --config merge.preferancestor="*"
  note: merging adfe50279922+ and 3b08d01b0ab5 using bids from ancestors 0f6b37dbe527 and 40663881a6dd
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: adfe50279922+, remote: 3b08d01b0ab5
   f1: remote unchanged -> k
   f2: versions differ -> m
  
  calculating bids for ancestor 40663881a6dd
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 40663881a6dd, local: adfe50279922+, remote: 3b08d01b0ab5
   f1: versions differ -> m
   f2: remote is newer -> g
  
  auction for merging merge bids
   f1: picking 'keep' action
   f2: picking 'get' action
  end of auction
  
   f2: remote is newer -> g
  getting f2
   f1: remote unchanged -> k
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ head *
  ==> f1 <==
  5 second change
  
  ==> f2 <==
  6 second change

Verify how the output looks and and how verbose it is:

  $ hg up -qC
  $ hg merge
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg up -qC tip
  $ hg merge -v
  note: merging 3b08d01b0ab5+ and adfe50279922 using bids from ancestors 0f6b37dbe527 and 40663881a6dd
  
  calculating bids for ancestor 0f6b37dbe527
  resolving manifests
  
  calculating bids for ancestor 40663881a6dd
  resolving manifests
  
  auction for merging merge bids
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
  getting f1
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)

  $ hg up -qC
  $ hg merge -v --debug --config merge.preferancestor="*"
  note: merging 3b08d01b0ab5+ and adfe50279922 using bids from ancestors 0f6b37dbe527 and 40663881a6dd
  
  calculating bids for ancestor 0f6b37dbe527
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 0f6b37dbe527, local: 3b08d01b0ab5+, remote: adfe50279922
   f1: remote is newer -> g
   f2: versions differ -> m
  
  calculating bids for ancestor 40663881a6dd
    searching for copies back to rev 3
  resolving manifests
   branchmerge: True, force: False, partial: False
   ancestor: 40663881a6dd, local: 3b08d01b0ab5+, remote: adfe50279922
   f1: versions differ -> m
   f2: remote unchanged -> k
  
  auction for merging merge bids
   f1: picking 'get' action
   f2: picking 'keep' action
  end of auction
  
   f1: remote is newer -> g
  getting f1
   f2: remote unchanged -> k
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
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

Verify that the old context ancestor works with / despite preferancestor:

  $ hg log -r 'ancestor(head())' --config merge.preferancestor=1 -T '{rev}\n'
  1
  $ hg log -r 'ancestor(head())' --config merge.preferancestor=2 -T '{rev}\n'
  2
  $ hg log -r 'ancestor(head())' --config merge.preferancestor=3 -T '{rev}\n'
  1
  $ hg log -r 'ancestor(head())' --config merge.preferancestor='1337 * - 2' -T '{rev}\n'
  2

  $ cd ..
