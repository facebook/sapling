#require serve

  $ echo "[extensions]" >> $HGRCPATH
  $ echo "fetch=" >> $HGRCPATH

test fetch with default branches only

  $ hg init a
  $ echo a > a/a
  $ hg --cwd a commit -Ama
  adding a
  $ hg clone a b
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone a c
  updating to branch default
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > a/b
  $ hg --cwd a commit -Amb
  adding b
  $ hg --cwd a parents -q
  1:d2ae7f538514

should pull one change

  $ hg --cwd b fetch ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files
  new changesets d2ae7f538514
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg --cwd b parents -q
  1:d2ae7f538514
  $ echo c > c/c
  $ hg --cwd c commit -Amc
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

  $ hg --cwd c fetch -d '0 0' -m 'automated merge' ../a
  pulling from ../a
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets d2ae7f538514
  updating to 2:d2ae7f538514
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:d36c0562f908
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 3:a323a0c43ec4 merges remote changes with local
  $ ls c
  a
  b
  c
  $ hg serve --cwd a -a localhost -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid
  $ HGPORT=`cat $TESTTMP/.port`
  $ cat a/hg.pid >> "$DAEMON_PIDS"

fetch over http, no auth
(this also tests that editor is invoked if '--edit' is specified)

  $ HGEDITOR=cat hg --cwd d fetch --edit http://localhost:$HGPORT/
  pulling from http://localhost:$HGPORT/ (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets d2ae7f538514
  updating to 2:d2ae7f538514
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:d36c0562f908
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  Automated merge with http://localhost:$HGPORT/ (glob)
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch merge
  HG: branch 'default'
  HG: changed c
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd d tip --template '{desc}\n'
  Automated merge with http://localhost:$HGPORT/ (glob)
  $ hg --cwd d status --rev 'tip^1' --rev tip
  A c
  $ hg --cwd d status --rev 'tip^2' --rev tip
  A b

fetch over http with auth (should be hidden in desc)
(this also tests that editor is not invoked if '--edit' is not
specified, even though commit message is not specified explicitly)

  $ HGEDITOR=cat hg --cwd e fetch http://user:password@localhost:$HGPORT/
  pulling from http://user:***@localhost:$HGPORT/ (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets d2ae7f538514
  updating to 2:d2ae7f538514
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  merging with 1:d36c0562f908
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd e tip --template '{desc}\n'
  Automated merge with http://localhost:$HGPORT/ (glob)
  $ hg clone a f
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg clone a g
  updating to branch default
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo f > f/f
  $ hg --cwd f ci -Amf
  adding f
  $ echo g > g/g
  $ hg --cwd g ci -Amg
  adding g
  $ hg clone -q f h
  $ hg clone -q g i

should merge f into g

  $ hg --cwd g fetch -d '0 0' --switch -m 'automated merge' ../f
  pulling from ../f
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 1 changes to 1 files (+1 heads)
  new changesets 6343ca3eff20
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging with 3:6343ca3eff20
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  new changeset 4:f7faa0b7d3c6 merges remote changes with local
  $ rm i/g

should abort, because i is modified

  $ hg --cwd i fetch ../h
  abort: uncommitted changes
  [255]

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
  new changesets 7837755a2789
  updating to 2:7837755a2789
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  merging with 1:d1f0c6c48ebd
  merging a
  0 files updated, 1 files merged, 0 files removed, 0 files unresolved
  new changeset 3:* merges remote changes with local (glob)
  $ hg --cwd i1726r2 heads default --template '{rev}\n'
  3

