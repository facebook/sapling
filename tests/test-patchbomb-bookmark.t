Create @ bookmark as main reference

  $ hg init repo
  $ cd repo
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "patchbomb=" >> $HGRCPATH
  $ hg book @

Create a dummy revision that must never be exported

  $ echo no > no
  $ hg ci -Amno -d '6 0'
  adding no

Create a feature and use -B

  $ hg book booktest
  $ echo first > a
  $ hg ci -Amfirst -d '7 0'
  adding a
  $ echo second > b
  $ hg ci -Amsecond -d '8 0'
  adding b
  $ hg email --date '1981-1-1 0:1' -n -t foo -s bookmark -B booktest
  From [test]: test
  this patch series consists of 2 patches.
  
  
  Write the introductory message for the patch series.
  
  Cc: 
  
  displaying [PATCH 0 of 2] bookmark ...
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 0 of 2] bookmark
  Message-Id: <patchbomb.347155260@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1981 00:01:00 +0000
  From: test
  To: foo
  
  
  displaying [PATCH 1 of 2] first ...
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 1 of 2] first
  X-Mercurial-Node: accde9b8b6dce861c185d0825c1affc09a79cb26
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 2
  Message-Id: <accde9b8b6dce861c185.347155261@*> (glob)
  X-Mercurial-Series-Id: <accde9b8b6dce861c185.347155261@*> (glob)
  In-Reply-To: <patchbomb.347155260@*> (glob)
  References: <patchbomb.347155260@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1981 00:01:01 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 7 0
  #      Thu Jan 01 00:00:07 1970 +0000
  # Node ID accde9b8b6dce861c185d0825c1affc09a79cb26
  # Parent  043bd3889e5aaf7d88fe3713cf425f782ad2fb71
  first
  
  diff -r 043bd3889e5a -r accde9b8b6dc a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:07 1970 +0000
  @@ -0,0 +1,1 @@
  +first
  
  displaying [PATCH 2 of 2] second ...
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH 2 of 2] second
  X-Mercurial-Node: 417defd1559c396ba06a44dce8dc1c2d2d653f3f
  X-Mercurial-Series-Index: 2
  X-Mercurial-Series-Total: 2
  Message-Id: <417defd1559c396ba06a.347155262@*> (glob)
  X-Mercurial-Series-Id: <accde9b8b6dce861c185.347155261@*> (glob)
  In-Reply-To: <patchbomb.347155260@*> (glob)
  References: <patchbomb.347155260@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Thu, 01 Jan 1981 00:01:02 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 8 0
  #      Thu Jan 01 00:00:08 1970 +0000
  # Node ID 417defd1559c396ba06a44dce8dc1c2d2d653f3f
  # Parent  accde9b8b6dce861c185d0825c1affc09a79cb26
  second
  
  diff -r accde9b8b6dc -r 417defd1559c b
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/b	Thu Jan 01 00:00:08 1970 +0000
  @@ -0,0 +1,1 @@
  +second
  
Do the same and combine with -o only one must be exported

  $ cd ..
  $ hg clone repo repo2
  updating to bookmark @
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ cd repo
  $ hg up @
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
  (activating bookmark @)
  $ hg book outgoing
  $ echo 1 > x
  $ hg ci -Am1 -d '8 0'
  adding x
  created new head
  $ hg push ../repo2 -B outgoing
  pushing to ../repo2
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  exporting bookmark outgoing
  $ echo 2 > y
  $ hg ci -Am2 -d '9 0'
  adding y
  $ hg email --date '1982-1-1 0:1' -n -t foo -s bookmark -B outgoing -o ../repo2
  comparing with ../repo2
  From [test]: test
  this patch series consists of 1 patches.
  
  Cc: 
  
  displaying [PATCH] bookmark ...
  MIME-Version: 1.0
  Content-Type: text/plain; charset="us-ascii"
  Content-Transfer-Encoding: 7bit
  Subject: [PATCH] bookmark
  X-Mercurial-Node: 8dab2639fd35f1e337ad866c372a5c44f1064e3c
  X-Mercurial-Series-Index: 1
  X-Mercurial-Series-Total: 1
  Message-Id: <8dab2639fd35f1e337ad.378691260@*> (glob)
  X-Mercurial-Series-Id: <8dab2639fd35f1e337ad.378691260@*> (glob)
  User-Agent: Mercurial-patchbomb/* (glob)
  Date: Fri, 01 Jan 1982 00:01:00 +0000
  From: test
  To: foo
  
  # HG changeset patch
  # User test
  # Date 9 0
  #      Thu Jan 01 00:00:09 1970 +0000
  # Node ID 8dab2639fd35f1e337ad866c372a5c44f1064e3c
  # Parent  0b24b8316483bf30bfc3e4d4168e922b169dbe66
  2
  
  diff -r 0b24b8316483 -r 8dab2639fd35 y
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/y	Thu Jan 01 00:00:09 1970 +0000
  @@ -0,0 +1,1 @@
  +2
  
