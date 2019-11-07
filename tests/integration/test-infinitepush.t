  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ cd $TESTTMP

setup common configuration for these tests
  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > amend=
  > infinitepush=
  > commitcloud=
  > EOF

setup repo

  $ hginit_treemanifest repo-hg
  $ cd repo-hg
  $ touch a && hg addremove && hg ci -q -ma
  adding a
  $ hg log -T '{short(node)}\n'
  3903775176ed

create master bookmark
  $ hg bookmark master_bookmark -r tip

  $ cd $TESTTMP

setup repo-push and repo-pull
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-push --noupdate
  $ hgclone_treemanifest ssh://user@dummy/repo-hg repo-pull --noupdate

blobimport

  $ blobimport repo-hg/.hg repo

start mononoke

  $ mononoke
  $ wait_for_mononoke $TESTTMP/repo


Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hg up tip
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new > newfile
  $ hg addremove -q
  $ hg ci -m new
  $ hgmn push ssh://user@dummy/repo -r . --bundle-store --debug --allow-anon
  pushing to ssh://user@dummy/repo
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  query 1; heads
  sending batch command
  searching for changes
  all remote heads known locally
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  checking for updated bookmarks
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes
  1 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  bundle2-input-bundle: 1 params no-transaction
  bundle2-input-part: "reply:changegroup" (params: 2 mandatory) supported
  bundle2-input-bundle: 0 parts total
  preparing listkeys for "phases"
  sending listkeys command
  received listkey for "phases": 0 bytes
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

  $ tglogp
  @  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hgmn pull -r 47da8b81097c5534f3eb7947a8764dd323cffe3d
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 47da8b81097c
  $ hgmn up -q 47da8b81097c
  $ cat newfile
  new

  $ tglogp
  @  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Do infinitepush (aka commit cloud) push, to a bookmark
  $ cd ../repo-push
  $ hg up tip
  0 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo new2 > newfile2
  $ hg addremove -q
  $ hg ci -m new2
  $ hgmn push ssh://user@dummy/repo -r . --to "scratch/123"
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing an infinitepush
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Unknown bookmark: scratch/123. Use --create to create one.",
  remote:     }
  remote:   Caused by:
  remote:     While verifying Infinite Push bookmark push
  remote:   Caused by:
  remote:     Unknown bookmark: scratch/123. Use --create to create one.
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ hgmn push ssh://user@dummy/repo -r . --to "scratch/123" --create
  pushing to ssh://user@dummy/repo
  searching for changes
  $ tglogp
  @  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind, HEX(changeset_id) FROM bookmarks;'
  master_bookmark|pull_default|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793
  scratch/123|scratch|58C64A8A96ADD9087220CA5B94CD892364562F40CBDA51ACFBBA2DAD8F5C979E
  $ hgmn push ssh://user@dummy/repo -r 3903775176ed --to "scratch/123"
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing an infinitepush
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "Non fastforward bookmark move (try --force?)",
  remote:     }
  remote:   Caused by:
  remote:     While verifying Infinite Push bookmark push
  remote:   Caused by:
  remote:     Non fastforward bookmark move (try --force?)
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]
  $ hgmn push ssh://user@dummy/repo -r 3903775176ed --to "scratch/123" --force
  pushing to ssh://user@dummy/repo
  searching for changes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind, HEX(changeset_id) FROM bookmarks;'
  master_bookmark|pull_default|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793
  scratch/123|scratch|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793
  $ hgmn push ssh://user@dummy/repo -r 007299f6399f --to "scratch/123"
  pushing to ssh://user@dummy/repo
  searching for changes
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind, HEX(changeset_id) FROM bookmarks;'
  master_bookmark|pull_default|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793
  scratch/123|scratch|58C64A8A96ADD9087220CA5B94CD892364562F40CBDA51ACFBBA2DAD8F5C979E
  $ hgmn push ssh://user@dummy/repo -r 007299f6399f --to "scratch/123" --create --config "infinitepush.branchpattern=foo"
  pushing rev 007299f6399f to destination ssh://user@dummy/repo bookmark scratch/123
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote:   Root cause:
  remote:     ErrorMessage {
  remote:         msg: "[push] Only Infinitepush bookmarks are allowed to match pattern ^scratch/.+$",
  remote:     }
  remote:   Caused by:
  remote:     [push] Only Infinitepush bookmarks are allowed to match pattern ^scratch/.+$
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]


  $ cd ../repo-pull
  $ hgmn pull -B "scratch/123"
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 007299f6399f
  $ hgmn up -q "007299f6399f"
  $ cat newfile2
  new2

  $ tglogp
  @  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  
  $ hg book --remote
     default/master_bookmark   0:3903775176ed
     default/scratch/123       2:007299f6399f

Pushbackup also works
  $ cd ../repo-push
  $ echo aa > aa && hg addremove && hg ci -q -m newrepo
  adding aa
  $ hgmn cloud backup --dest ssh://user@dummy/repo --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  sending knownnodes command
  sending knownnodes command
  backing up stack rooted at 47da8b81097c
  3 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  007299f6399f84ad9c3b269137902d47d908936d
  2cfeca6399fdb0084a6eba69275ea7aeb1d07667
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 4 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "pushvars" (params: 1 advisory) empty payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 1 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "pushvars" (params: 1 advisory) empty payload
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  commitcloud: backed up 1 commit

  $ tglogp
  @  3: 2cfeca6399fd draft 'newrepo'
  |
  o  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ cd ../repo-pull
  $ hgmn pull -r 2cfeca6399fdb0084a6eba69275ea7aeb1d07667
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 2cfeca6399fd
  $ hgmn up -q 2cfeca6399fd
  $ cat aa
  aa

  $ tglogp
  @  3: 2cfeca6399fd draft 'newrepo'
  |
  o  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Pushbackup that pushes only bookmarks
  $ cd ../repo-push
  $ hg book newbook
  $ hgmn cloud backup --dest ssh://user@dummy/repo --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "pushvars" (params: 1 advisory) empty payload
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  nothing to back up

  $ tglogp
  @  3: 2cfeca6399fd draft 'newrepo' newbook
  |
  o  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Finally, try to push existing commit to a public bookmark
  $ hgmn push -r . --to master_bookmark
  pushing rev 2cfeca6399fd to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

  $ tglogp
  @  3: 2cfeca6399fd public 'newrepo' newbook
  |
  o  2: 007299f6399f public 'new2'
  |
  o  1: 47da8b81097c public 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  


Check phases on another side (for pull command and pull -r)
  $ cd ../repo-pull
  $ hgmn pull -r 47da8b81097c5534f3eb7947a8764dd323cffe3d
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files

  $ tglogp
  @  3: 2cfeca6399fd draft 'newrepo'
  |
  o  2: 007299f6399f draft 'new2'
  |
  o  1: 47da8b81097c public 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files

  $ tglogp
  @  3: 2cfeca6399fd public 'newrepo'
  |
  o  2: 007299f6399f public 'new2'
  |
  o  1: 47da8b81097c public 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

# Test phases a for stack that is partially public
  $ cd ../repo-push
  $ hgmn up 3903775176ed
  0 files updated, 0 files merged, 3 files removed, 0 files unresolved
  (leaving bookmark newbook)
  $ echo new > file1
  $ hg addremove -q
  $ hg ci -m "feature release"

  $ hgmn push -r . --to "test_release_1.0.0"  --create # push this release (creating new remote bookmark)
  pushing rev 500658c138a4 to destination ssh://user@dummy/repo bookmark test_release_1.0.0
  searching for changes
  exporting bookmark test_release_1.0.0
  $ echo new > file2
  $ hg addremove -q
  $ hg ci -m "change on top of the release"
  $ hgmn cloud backup --dest ssh://user@dummy/repo
  backing up stack rooted at eca836c7c651
  commitcloud: backed up 1 commit

  $ tglogp
  @  5: eca836c7c651 draft 'change on top of the release'
  |
  o  4: 500658c138a4 public 'feature release'
  |
  | o  3: 2cfeca6399fd public 'newrepo' newbook
  | |
  | o  2: 007299f6399f public 'new2'
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  
 
  $ hg log -r . -T '{node}\n'
  eca836c7c6519b769367cc438ce09d83b4a4e8e1

  $ cd ../repo-pull
  $ hgmn pull -r eca836c7c6519b769367cc438ce09d83b4a4e8e1 # draft revision based on different public bookmark
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files (+1 heads)
  new changesets 500658c138a4:eca836c7c651

  $ tglogp
  o  5: eca836c7c651 draft 'change on top of the release'
  |
  o  4: 500658c138a4 public 'feature release'
  |
  | @  3: 2cfeca6399fd public 'newrepo'
  | |
  | o  2: 007299f6399f public 'new2'
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ hgmn pull -r test_release_1.0.0
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files

  $ tglogp
  o  5: eca836c7c651 draft 'change on top of the release'
  |
  o  4: 500658c138a4 public 'feature release'
  |
  | @  3: 2cfeca6399fd public 'newrepo'
  | |
  | o  2: 007299f6399f public 'new2'
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  
 

Test phases with pushrebase
  $ cd ../repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > EOF
  $ hg up 3903775176ed -q 
  $ echo new > filea
  $ hg addremove -q
  $ hg ci -m "new feature on top of master"
  $ hgmn push -r . --to master_bookmark # push-rebase
  pushing rev f9e4cd522499 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  updating bookmark master_bookmark

  $ tglogp
  o  7: 1708c61178dd public 'new feature on top of master'
  |
  | @  6: f9e4cd522499 draft 'new feature on top of master'
  | |
  | | o  5: eca836c7c651 draft 'change on top of the release'
  | | |
  | | o  4: 500658c138a4 public 'feature release'
  | |/
  o |  3: 2cfeca6399fd public 'newrepo' newbook
  | |
  o |  2: 007299f6399f public 'new2'
  | |
  o |  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  

Repos clean up
  $ cd ../repo-push
  $ hg hide -r "draft()" -q
  $ cd ../repo-pull
  $ hg hide -r "draft()" -q


More sophisticated test for phases
  $ cd ../repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=!
  > EOF

  $ hgmn up 1708c61178dd -q
  $ mkcommit ww
  $ hgmn push -r . --to "release 1"  --create -q
  $ mkcommit xx
  $ mkcommit yy
  $ mkcommit zz

  $ hgmn up 1708c61178dd -q
  $ mkcommit www
  $ mkcommit xxx
  $ hgmn push -r . --to "release 2"  --create -q
  $ mkcommit yyy
  $ mkcommit zzz

  $ hgmn up 1708c61178dd -q
  $ mkcommit wwww
  $ mkcommit xxxx
  $ mkcommit yyyy
  $ hgmn push -r . --to "release 3"  --create -q
  $ mkcommit zzzz

  $ hgmn up 1708c61178dd -q
  $ mkcommit wwwww
  $ mkcommit xxxxx
  $ mkcommit yyyyy
  $ mkcommit zzzzz
  $ hgmn push -r . --to "release 4"  --create -q

  $ hgmn cloud backup --dest ssh://user@dummy/repo -q

  $ hgmn cloud check --dest ssh://user@dummy/repo -r 7d67c7248d48 --remote
  7d67c7248d486cb264270530ef906f1d09d6c650 backed up
  $ hgmn cloud check --dest ssh://user@dummy/repo -r bf677f20a49d --remote
  bf677f20a49dc5ac94946f3d91ad181f8a6fdbab backed up
  $ hgmn cloud check --dest ssh://user@dummy/repo -r 5e59ac0f4dd0 --remote
  5e59ac0f4dd00fd4d751f9f3663be99df0f4765d backed up

  $ tglogp
  @  23: b9f080ea9500 public 'zzzzz'
  |
  o  22: 6e068f112af8 public 'yyyyy'
  |
  o  21: 0ff6f97758ae public 'xxxxx'
  |
  o  20: 8be205326fcf public 'wwwww'
  |
  | o  19: 7d67c7248d48 draft 'zzzz'
  | |
  | o  18: 859e9fdde968 public 'yyyy'
  | |
  | o  17: abe01677f4a6 public 'xxxx'
  | |
  | o  16: 4710fc0238de public 'wwww'
  |/
  | o  15: bf677f20a49d draft 'zzz'
  | |
  | o  14: 43db2471732d draft 'yyy'
  | |
  | o  13: f743965444d9 public 'xxx'
  | |
  | o  12: 83da839eb4d2 public 'www'
  |/
  | o  11: 5e59ac0f4dd0 draft 'zz'
  | |
  | o  10: 1a4fd3035391 draft 'yy'
  | |
  | o  9: c2234433b092 draft 'xx'
  | |
  | o  8: 2ba1f5f6cccd public 'ww'
  |/
  o  7: 1708c61178dd public 'new feature on top of master'
  |
  | o  4: 500658c138a4 public 'feature release'
  | |
  o |  3: 2cfeca6399fd public 'newrepo' newbook
  | |
  o |  2: 007299f6399f public 'new2'
  | |
  o |  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  
At the moment short hahses are not working, print the full hashes to use then in hg pull command
  $ hg log -T '{node}\n' -r 'heads(all())'
  500658c138a447f7ba651547d45ac510ae4e6db2
  5e59ac0f4dd00fd4d751f9f3663be99df0f4765d
  bf677f20a49dc5ac94946f3d91ad181f8a6fdbab
  7d67c7248d486cb264270530ef906f1d09d6c650
  b9f080ea95005f3513a22aa15f1f74d7371ce5d4

  $ cd ../repo-pull

  $ hgmn cloud restorebackup
  abort: 'listkeyspatterns' command is not supported for the server ssh://user@dummy/repo
  [255]

  $ hgmn pull -r 5e59ac0f4dd00fd4d751f9f3663be99df0f4765d -r bf677f20a49dc5ac94946f3d91ad181f8a6fdbab -r 7d67c7248d486cb264270530ef906f1d09d6c650 -r b9f080ea95005f3513a22aa15f1f74d7371ce5d4 -q

  $ tglogpnr -r "::b9f080ea95005f3513a22aa15f1f74d7371ce5d4 - ::default/master_bookmark"
  o  b9f080ea9500 public 'zzzzz'
  |
  o  6e068f112af8 public 'yyyyy'
  |
  o  0ff6f97758ae public 'xxxxx'
  |
  o  8be205326fcf public 'wwwww'
  |
  ~
  $ tglogpnr -r "::7d67c7248d486cb264270530ef906f1d09d6c650 - ::default/master_bookmark"
  o  7d67c7248d48 draft 'zzzz'
  |
  o  859e9fdde968 public 'yyyy'
  |
  o  abe01677f4a6 public 'xxxx'
  |
  o  4710fc0238de public 'wwww'
  |
  ~
  $ tglogpnr -r "::bf677f20a49dc5ac94946f3d91ad181f8a6fdbab - ::default/master_bookmark"
  o  bf677f20a49d draft 'zzz'
  |
  o  43db2471732d draft 'yyy'
  |
  o  f743965444d9 public 'xxx'
  |
  o  83da839eb4d2 public 'www'
  |
  ~
  $ tglogpnr -r "::5e59ac0f4dd00fd4d751f9f3663be99df0f4765d - ::default/master_bookmark"
  o  5e59ac0f4dd0 draft 'zz'
  |
  o  1a4fd3035391 draft 'yy'
  |
  o  c2234433b092 draft 'xx'
  |
  o  2ba1f5f6cccd public 'ww'
  |
  ~
