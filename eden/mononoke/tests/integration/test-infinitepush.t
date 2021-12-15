# Copyright (c) Facebook, Inc. and its affiliates.
#
# This software may be used and distributed according to the terms of the
# GNU General Public License found in the LICENSE file in the root
# directory of this source tree.

  $ . "${TEST_FIXTURES}/library.sh"

setup configuration
  $ INFINITEPUSH_NAMESPACE_REGEX='^scratch/.+$' setup_common_config
  $ merge_tunables <<EOF
  > {
  >   "killswitches": {
  >     "mutation_accept_for_infinitepush": true,
  >     "mutation_advertise_for_infinitepush": true,
  >     "mutation_generate_for_draft": true
  >   },
  >   "ints": {
  >     "zstd_compression_level": 3
  >   }
  > }
  > EOF
  $ cd $TESTTMP

setup common configuration for these tests

  $ enable amend infinitepush commitcloud remotenames undo

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
  $ wait_for_mononoke


Do infinitepush (aka commit cloud) push
  $ cd repo-push
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > httpbookmarks=True
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
  local heads: 1; remote heads: 1 (explicit: 0); initial common: 1
  1 total queries in 0.0000s
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
  preparing listkeys for "bookmarks"
  sending listkeys command
  received listkey for "bookmarks": 57 bytes

  $ tglogp
  @  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  

  $ cd ../repo-pull
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > remotenames=
  > [infinitepush]
  > server=False
  > branchpattern=re:scratch/.+
  > EOF
  $ hgmn pull -r 47da8b81097c
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up -q 47da8b81097c
  $ cat newfile
  new

  $ tglogp
  @  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  

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
  remote: 
  remote:   Root cause:
  remote:     Unknown bookmark: scratch/123. Use --create to create one.
  remote: 
  remote:   Caused by:
  remote:     Unknown bookmark: scratch/123. Use --create to create one.
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing an infinitepush",
  remote:         source: "Unknown bookmark: scratch/123. Use --create to create one.",
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]

  $ hgmn push ssh://user@dummy/repo -r . --to "scratch/123" --create
  pushing to ssh://user@dummy/repo
  searching for changes
  $ tglogp
  @  007299f6399f draft 'new2'
  │
  o  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  
  $ sqlite3 "$TESTTMP/monsql/sqlite_dbs" 'SELECT name, hg_kind, HEX(changeset_id) FROM bookmarks;'
  master_bookmark|pull_default|E10EC6CD13B1CBCFE2384F64BD37FC71B4BF9CFE21487D2EAF5064C1B3C0B793
  scratch/123|scratch|58C64A8A96ADD9087220CA5B94CD892364562F40CBDA51ACFBBA2DAD8F5C979E
  $ hgmn push ssh://user@dummy/repo -r 3903775176ed --to "scratch/123"
  pushing to ssh://user@dummy/repo
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing an infinitepush
  remote: 
  remote:   Root cause:
  remote:     Non fast-forward bookmark move from * to * (glob)
  remote: 
  remote:   Caused by:
  remote:     Failed to fast-forward scratch bookmark (try --force?)
  remote:   Caused by:
  remote:     Non fast-forward bookmark move from 58c64a8a96add9087220ca5b94cd892364562f40cbda51acfbba2dad8f5c979e to e10ec6cd13b1cbcfe2384f64bd37fc71b4bf9cfe21487d2eaf5064c1b3c0b793
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing an infinitepush",
  remote:         source: Error {
  remote:             context: "Failed to fast-forward scratch bookmark (try --force?)",
  remote:             source: NonFastForwardMove {
  remote:                 from: ChangesetId(
  remote:                     Blake2(58c64a8a96add9087220ca5b94cd892364562f40cbda51acfbba2dad8f5c979e),
  remote:                 ),
  remote:                 to: ChangesetId(
  remote:                     Blake2(e10ec6cd13b1cbcfe2384f64bd37fc71b4bf9cfe21487d2eaf5064c1b3c0b793),
  remote:                 ),
  remote:             },
  remote:         },
  remote:     }
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
  $ hgmn push ssh://user@dummy/repo -r 007299f6399f --to "scratch/124" --create --config "infinitepush.branchpattern=foo"
  pushing rev 007299f6399f to destination ssh://user@dummy/repo bookmark scratch/124
  searching for changes
  remote: Command failed
  remote:   Error:
  remote:     While doing a push
  remote: 
  remote:   Root cause:
  remote:     Invalid public bookmark: scratch/124 (only scratch bookmarks may match pattern ^scratch/.+$)
  remote: 
  remote:   Caused by:
  remote:     Failed to create bookmark
  remote:   Caused by:
  remote:     Invalid public bookmark: scratch/124 (only scratch bookmarks may match pattern ^scratch/.+$)
  remote: 
  remote:   Debug context:
  remote:     Error {
  remote:         context: "While doing a push",
  remote:         source: Error {
  remote:             context: "Failed to create bookmark",
  remote:             source: InvalidPublicBookmark {
  remote:                 bookmark: BookmarkName {
  remote:                     bookmark: "scratch/124",
  remote:                 },
  remote:                 pattern: "^scratch/.+$",
  remote:             },
  remote:         },
  remote:     }
  abort: stream ended unexpectedly (got 0 bytes, expected 4)
  [255]


  $ cd ../repo-pull
  $ hgmn pull -B "scratch/123"
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up -q "007299f6399f"
  $ cat newfile2
  new2

  $ tglogp
  @  007299f6399f draft 'new2'
  │
  o  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  
  $ hg book --remote
     default/master_bookmark   3903775176ed
     default/scratch/123       007299f6399f

Pushbackup also works
  $ cd ../repo-push
  $ echo aa > aa && hg addremove && hg ci -q -m newrepo
  adding aa
  $ hgmn cloud backup --debug
  running * (glob)
  sending hello command
  sending between command
  remote: * (glob)
  remote: capabilities: * (glob)
  remote: 1
  sending clienttelemetry command
  sending knownnodes command
  reusing connection from pool
  sending knownnodes command
  backing up stack rooted at 47da8b81097c
  reusing connection from pool
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
  commitcloud: backed up 1 commit

Pushbackup to mononoke peer with compression enabled
(a larger file is needed to repro problems with zstd compression)
  $ dd if=/dev/zero of=aa bs=4048 count=1024 2> /dev/null
  $ hg amend -m "xxx"
  $ MONONOKE_DIRECT_PEER=1 hgmn cloud backup --config infinitepush.bundlecompression=ZS --config mononokepeer.compression=true
  backing up stack rooted at 47da8b81097c
  commitcloud: backed up 1 commit

  $ grep "Root cause: unconsumed data" "$TESTTMP/mononoke.out"
  [1]

  $ hg undo
  undone to * before amend -m xxx (glob)
  $ tglogp
  @  2cfeca6399fd draft 'newrepo'
  │
  o  007299f6399f draft 'new2'
  │
  o  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  

  $ cd ../repo-pull
  $ hgmn pull -r 2cfeca6399fd
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ hgmn up -q 2cfeca6399fd
  $ cat aa
  aa

  $ tglogp
  @  2cfeca6399fd draft 'newrepo'
  │
  o  007299f6399f draft 'new2'
  │
  o  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  

Pushbackup that does nothing, as only bookmarks have changed
  $ cd ../repo-push
  $ hg book newbook
  $ hgmn cloud backup --debug
  nothing to back up

  $ tglogp
  @  2cfeca6399fd draft 'newrepo' newbook
  │
  o  007299f6399f draft 'new2'
  │
  o  47da8b81097c draft 'new'
  │
  o  3903775176ed public 'a'
  

Finally, try to push existing commit to a public bookmark
  $ hgmn push -r . --to master_bookmark
  pushing rev 2cfeca6399fd to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

  $ tglogp
  @  2cfeca6399fd public 'newrepo' newbook
  │
  o  007299f6399f public 'new2'
  │
  o  47da8b81097c public 'new'
  │
  o  3903775176ed public 'a'
  


Check phases on another side (for pull command and pull -r)
  $ cd ../repo-pull
  $ hgmn pull -r 47da8b81097c
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes

  $ tglogp
  @  2cfeca6399fd public 'newrepo'
  │
  o  007299f6399f public 'new2'
  │
  o  47da8b81097c public 'new'
  │
  o  3903775176ed public 'a'
  

  $ hgmn pull
  pulling from ssh://user@dummy/repo
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes

  $ tglogp
  @  2cfeca6399fd public 'newrepo'
  │
  o  007299f6399f public 'new2'
  │
  o  47da8b81097c public 'new'
  │
  o  3903775176ed public 'a'
  

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
  $ hgmn cloud backup
  backing up stack rooted at eca836c7c651
  commitcloud: backed up 1 commit

  $ tglogp
  @  eca836c7c651 draft 'change on top of the release'
  │
  o  500658c138a4 public 'feature release'
  │
  │ o  2cfeca6399fd public 'newrepo' newbook
  │ │
  │ o  007299f6399f public 'new2'
  │ │
  │ o  47da8b81097c public 'new'
  ├─╯
  o  3903775176ed public 'a'
  
 
  $ hg log -r . -T '{node}\n'
  eca836c7c6519b769367cc438ce09d83b4a4e8e1

  $ cd ../repo-pull
  $ hgmn pull -r eca836c7c651 # draft revision based on different public bookmark
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes

  $ tglogp
  o  eca836c7c651 draft 'change on top of the release'
  │
  o  500658c138a4 public 'feature release'
  │
  │ @  2cfeca6399fd public 'newrepo'
  │ │
  │ o  007299f6399f public 'new2'
  │ │
  │ o  47da8b81097c public 'new'
  ├─╯
  o  3903775176ed public 'a'
  

  $ hgmn pull -r test_release_1.0.0
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  adding manifests
  adding file changes

  $ tglogp
  o  eca836c7c651 draft 'change on top of the release'
  │
  o  500658c138a4 public 'feature release'
  │
  │ @  2cfeca6399fd public 'newrepo'
  │ │
  │ o  007299f6399f public 'new2'
  │ │
  │ o  47da8b81097c public 'new'
  ├─╯
  o  3903775176ed public 'a'
  
 

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
  updating bookmark master_bookmark

  $ tglogp
  o  1708c61178dd public 'new feature on top of master'
  │
  │ @  f9e4cd522499 draft 'new feature on top of master'
  │ │
  │ │ o  eca836c7c651 draft 'change on top of the release'
  │ │ │
  │ │ o  500658c138a4 public 'feature release'
  │ ├─╯
  o │  2cfeca6399fd public 'newrepo' newbook
  │ │
  o │  007299f6399f public 'new2'
  │ │
  o │  47da8b81097c public 'new'
  ├─╯
  o  3903775176ed public 'a'
  

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

  $ hgmn cloud backup -q

  $ hgmn cloud check -r 7d67c7248d48 --remote
  7d67c7248d486cb264270530ef906f1d09d6c650 backed up
  $ hgmn cloud check -r bf677f20a49d --remote
  bf677f20a49dc5ac94946f3d91ad181f8a6fdbab backed up
  $ hgmn cloud check -r 5e59ac0f4dd0 --remote
  5e59ac0f4dd00fd4d751f9f3663be99df0f4765d backed up

  $ tglogp
  @  b9f080ea9500 public 'zzzzz'
  │
  o  6e068f112af8 public 'yyyyy'
  │
  o  0ff6f97758ae public 'xxxxx'
  │
  o  8be205326fcf public 'wwwww'
  │
  │ o  7d67c7248d48 draft 'zzzz'
  │ │
  │ o  859e9fdde968 public 'yyyy'
  │ │
  │ o  abe01677f4a6 public 'xxxx'
  │ │
  │ o  4710fc0238de public 'wwww'
  ├─╯
  │ o  bf677f20a49d draft 'zzz'
  │ │
  │ o  43db2471732d draft 'yyy'
  │ │
  │ o  f743965444d9 public 'xxx'
  │ │
  │ o  83da839eb4d2 public 'www'
  ├─╯
  │ o  5e59ac0f4dd0 draft 'zz'
  │ │
  │ o  1a4fd3035391 draft 'yy'
  │ │
  │ o  c2234433b092 draft 'xx'
  │ │
  │ o  2ba1f5f6cccd public 'ww'
  ├─╯
  o  1708c61178dd public 'new feature on top of master'
  │
  │ o  500658c138a4 public 'feature release'
  │ │
  o │  2cfeca6399fd public 'newrepo' newbook
  │ │
  o │  007299f6399f public 'new2'
  │ │
  o │  47da8b81097c public 'new'
  ├─╯
  o  3903775176ed public 'a'
  

  $ cd ../repo-pull

  $ hgmn pull -r b  # test ambiguous prefix
  pulling from ssh://user@dummy/repo
  abort: ambiguous identifier
  suggestions are:
  
  changeset: bf677f20a49dc5ac94946f3d91ad181f8a6fdbab
  author: test
  date: Thu, 01 Jan 1970 00:00:00 +0000
  summary: zzz
  
  changeset: b9f080ea95005f3513a22aa15f1f74d7371ce5d4
  author: test
  date: Thu, 01 Jan 1970 00:00:00 +0000
  summary: zzzzz
  !
  [255]

  $ hgmn pull -r 5e59ac0f4dd0 -r bf677f20a49d -r 7d67c7248d48 -r b9f080ea9500 -q

  $ tglogpnr -r "::b9f080ea9500 - ::default/master_bookmark"
  o  b9f080ea9500 public 'zzzzz'  default/release 4
  │
  o  6e068f112af8 public 'yyyyy'
  │
  o  0ff6f97758ae public 'xxxxx'
  │
  o  8be205326fcf public 'wwwww'
  │
  ~
  $ tglogpnr -r "::7d67c7248d48 - ::default/master_bookmark"
  o  7d67c7248d48 draft 'zzzz'
  │
  o  859e9fdde968 public 'yyyy'  default/release 3
  │
  o  abe01677f4a6 public 'xxxx'
  │
  o  4710fc0238de public 'wwww'
  │
  ~
  $ tglogpnr -r "::bf677f20a49d - ::default/master_bookmark"
  o  bf677f20a49d draft 'zzz'
  │
  o  43db2471732d draft 'yyy'
  │
  o  f743965444d9 public 'xxx'  default/release 2
  │
  o  83da839eb4d2 public 'www'
  │
  ~
  $ tglogpnr -r "::5e59ac0f4dd0 - ::default/master_bookmark"
  o  5e59ac0f4dd0 draft 'zz'
  │
  o  1a4fd3035391 draft 'yy'
  │
  o  c2234433b092 draft 'xx'
  │
  o  2ba1f5f6cccd public 'ww'  default/release 1
  │
  ~
