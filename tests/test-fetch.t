adjust to non-default HGPORT, e.g. with run-tests.py -j

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "fetch=" >> $HGRCPATH

test fetch with default branches only

  $ hg init a
  $ echo a > a/a
  $ hg --cwd a commit -d '1 0' -Ama
  adding a
  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone a c
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > a/b
  $ hg --cwd a commit -d '2 0' -Amb
  adding b
  $ hg --cwd a parents -q
  1:97d72e5f12c7

should pull one change

  $ hg --cwd b fetch ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd b parents -q
  1:97d72e5f12c7
  $ echo c > c/c
  $ hg --cwd c commit -d '3 0' -Amc
  adding c
  $ hg clone c d
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone c e
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved

We cannot use the default commit message if fetching from a local
repo, because the path of the repo will be included in the commit
message, making every commit appear different.
should merge c into a

  $ hg --cwd c fetch -d '4 0' -m 'automated merge' ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating to 2:97d72e5f12c7
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:5e056962225c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 3:cd3a41621cf0 merges remote changes with local
  $ ls c
  a
  b
  c
  $ netstat -tnap 2>/dev/null | grep $HGPORT | grep LISTEN
  [1]
  $ hg --cwd a serve -a localhost -p $HGPORT -d --pid-file=hg.pid
  $ cat a/hg.pid >> "$DAEMON_PIDS"

fetch over http, no auth

  $ hg --cwd d fetch -d '5 0' http://localhost:$HGPORT/
  pulling from http://localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating to 2:97d72e5f12c7
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:5e056962225c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd d tip --template '{desc}\n'
  Automated merge with http://localhost:$HGPORT/

fetch over http with auth (should be hidden in desc)

  $ hg --cwd e fetch -d '5 0' http://user:password@localhost:$HGPORT/
  pulling from http://user:***@localhost:$HGPORT/
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating to 2:97d72e5f12c7
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:5e056962225c
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd e tip --template '{desc}\n'
  Automated merge with http://localhost:$HGPORT/
  $ hg clone a f
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone a g
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f > f/f
  $ hg --cwd f ci -d '6 0' -Amf
  adding f
  $ echo g > g/g
  $ hg --cwd g ci -d '6 0' -Amg
  adding g
  $ hg clone -q f h
  $ hg clone -q g i

should merge f into g

  $ hg --cwd g fetch -d '7 0' --switch -m 'automated merge' ../f
  pulling from ../f
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging with 3:cc6a3744834d
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 4:55aa4f32ec59 merges remote changes with local
  $ rm i/g

should abort, because i is modified

  $ hg --cwd i fetch ../h
  abort: working directory is missing some files
  [255]

test fetch with named branches

  $ hg init nbase
  $ echo base > nbase/a
  $ hg -R nbase ci -d '1 0' -Am base
  adding a
  $ hg -R nbase branch a
  marked working directory as branch a
  $ echo a > nbase/a
  $ hg -R nbase ci -d '2 0' -m a
  $ hg -R nbase up -C 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R nbase branch b
  marked working directory as branch b
  $ echo b > nbase/b
  $ hg -R nbase ci -Ad '3 0' -m b
  adding b
  $ echo
  

pull in change on foreign branch

  $ hg clone nbase n1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone nbase n2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n1 up -C a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aa > n1/a
  $ hg -R n1 ci -d '4 0' -m a1
  $ hg -R n2 up -C b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n2 fetch -d '9 0' -m 'merge' n1
  pulling from n1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files

parent should be 2 (no automatic update)

  $ hg -R n2 parents --template '{rev}\n'
  2
  $ rm -fr n1 n2
  $ echo
  

pull in changes on both foreign and local branches

  $ hg clone nbase n1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone nbase n2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n1 up -C a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo aa > n1/a
  $ hg -R n1 ci -d '4 0' -m a1
  $ hg -R n1 up -C b
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bb > n1/b
  $ hg -R n1 ci -d '5 0' -m b1
  $ hg -R n2 up -C b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n2 fetch -d '9 0' -m 'merge' n1
  pulling from n1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 2 changes to 2 files
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

parent should be 4 (fast forward)

  $ hg -R n2 parents --template '{rev}\n'
  4
  $ rm -fr n1 n2
  $ echo
  

pull changes on foreign (2 new heads) and local (1 new head) branches
with a local change

  $ hg clone nbase n1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone nbase n2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n1 up -C a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo a1 > n1/a
  $ hg -R n1 ci -d '4 0' -m a1
  $ hg -R n1 up -C b
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo bb > n1/b
  $ hg -R n1 ci -d '5 0' -m b1
  $ hg -R n1 up -C 1
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  $ echo a2 > n1/a
  $ hg -R n1 ci -d '6 0' -m a2
  created new head
  $ hg -R n2 up -C b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo change >> n2/c
  $ hg -R n2 ci -Ad '7 0' -m local
  adding c
  $ hg -R n2 fetch -d '9 0' -m 'merge' n1
  pulling from n1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files (+2 heads)
  updating to 5:708c6cce3d26
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 3:d83427717b1f
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 7:48f1a33f52af merges remote changes with local

parent should be 7 (new merge changeset)

  $ hg -R n2 parents --template '{rev}\n'
  7
  $ rm -fr n1 n2

pull in changes on foreign (merge of local branch) and local (2 new
heads) with a local change

  $ hg clone nbase n1
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone nbase n2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n1 up -C a
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg -R n1 merge b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  (branch merge, don't forget to commit)
  $ hg -R n1 ci -d '4 0' -m merge
  $ hg -R n1 up -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo c > n1/a
  $ hg -R n1 ci -d '5 0' -m c
  $ hg -R n1 up -C 2
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo cc > n1/a
  $ hg -R n1 ci -d '6 0' -m cc
  created new head
  $ hg -R n2 up -C b
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo change >> n2/b
  $ hg -R n2 ci -Ad '7 0' -m local
  $ hg -R n2 fetch -d '9 0' -m 'merge' n1
  pulling from n1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 2 changes to 1 files (+2 heads)
  not merging with 1 other new branch heads (use "hg heads ." and "hg merge" to merge them)
  [1]

parent should be 3 (fetch did not merge anything)

  $ hg -R n2 parents --template '{rev}\n'
  3
  $ rm -fr n1 n2

pull in change on different branch than dirstate

  $ hg init n1
  $ echo a > n1/a
  $ hg -R n1 ci -Am initial
  adding a
  $ hg clone n1 n2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > n1/a
  $ hg -R n1 ci -m next
  $ hg -R n2 branch topic
  marked working directory as branch topic
  $ hg -R n2 fetch -d '0 0' -m merge n1
  abort: working dir not at branch tip (use "hg update" to check out branch tip)
  [255]

parent should be 0 (fetch did not update or merge anything)

  $ hg -R n2 parents --template '{rev}\n'
  0
  $ rm -fr n1 n2

test fetch with inactive branches

  $ hg init ib1
  $ echo a > ib1/a
  $ hg --cwd ib1 ci -Am base
  adding a
  $ hg --cwd ib1 branch second
  marked working directory as branch second
  $ echo b > ib1/b
  $ hg --cwd ib1 ci -Am onsecond
  adding b
  $ hg --cwd ib1 branch -f default
  marked working directory as branch default
  $ echo c > ib1/c
  $ hg --cwd ib1 ci -Am newdefault
  adding c
  created new head
  $ hg clone ib1 ib2
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved

fetch should succeed

  $ hg --cwd ib2 fetch ../ib1
  pulling from ../ib1
  searching for changes
  no changes found
  $ rm -fr ib1 ib2

test issue1726

  $ hg init i1726r1
  $ echo a > i1726r1/a
  $ hg --cwd i1726r1 ci -Am base
  adding a
  $ hg clone i1726r1 i1726r2
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > i1726r1/a
  $ hg --cwd i1726r1 ci -m second
  $ echo c > i1726r2/a
  $ hg --cwd i1726r2 ci -m third
  $ HGMERGE=true hg --cwd i1726r2 fetch ../i1726r1
  pulling from ../i1726r1
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  updating to 2:7837755a2789
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging with 1:d1f0c6c48ebd
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd i1726r2 heads default --template '{rev}\n'
  3
  $ echo
  

test issue2047

  $ hg -q init i2047a
  $ cd i2047a
  $ echo a > a
  $ hg -q ci -Am a
  $ hg -q branch stable
  $ echo b > b
  $ hg -q ci -Am b
  $ cd ..
  $ hg -q clone -r 0 i2047a i2047b
  $ cd i2047b
  $ hg fetch ../i2047a
  pulling from ../i2047a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  $ "$TESTDIR/killdaemons.py"
