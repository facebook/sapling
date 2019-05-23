  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
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
  2 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  95cad53aab1b0b33eceee14473b3983312721529
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
  @  2: 95cad53aab1b draft 'newrepo'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ cd ../repo-pull
  $ hgmn pull -r 95cad53aab1b0b33eceee14473b3983312721529
  pulling from ssh://user@dummy/repo
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 95cad53aab1b
  $ hgmn up -q 95cad53aab1b0b33ecee
  $ cat aa
  aa

  $ tglogp
  @  2: 95cad53aab1b draft 'newrepo'
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
  @  2: 95cad53aab1b draft 'newrepo' newbook
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Finally, try to push existing commit to a public bookmark
  $ hgmn push -r . --to master_bookmark
  pushing rev 95cad53aab1b to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  updating bookmark master_bookmark

  $ tglogp
  @  2: 95cad53aab1b public 'newrepo' newbook
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
  updating bookmark master_bookmark

  $ tglogp
  @  2: 95cad53aab1b draft 'newrepo' master_bookmark
  |
  o  1: 47da8b81097c public 'new'
  |
  o  0: 3903775176ed public 'a'
  

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
  @  2: 95cad53aab1b public 'newrepo' master_bookmark
  |
  o  1: 47da8b81097c public 'new'
  |
  o  0: 3903775176ed public 'a'
  

# Test phases a for stack that is partially public
  $ cd ../repo-push
  $ hgmn up 3903775176ed
  0 files updated, 0 files merged, 2 files removed, 0 files unresolved
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
  @  4: eca836c7c651 draft 'change on top of the release'
  |
  o  3: 500658c138a4 public 'feature release'
  |
  | o  2: 95cad53aab1b public 'newrepo' newbook
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
  adding remote bookmark test_release_1.0.0
  new changesets 500658c138a4:eca836c7c651

  $ tglogp
  o  4: eca836c7c651 draft 'change on top of the release'
  |
  o  3: 500658c138a4 public 'feature release' test_release_1.0.0
  |
  | @  2: 95cad53aab1b public 'newrepo' master_bookmark
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a'
  

  $ hgmn pull -r test_release_1.0.0
  pulling from ssh://user@dummy/repo
  no changes found
  adding changesets
  devel-warn: applied empty changegroup at: * (_processchangegroup) (glob)
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files

  $ tglogp
  o  4: eca836c7c651 draft 'change on top of the release'
  |
  o  3: 500658c138a4 public 'feature release' test_release_1.0.0
  |
  | @  2: 95cad53aab1b public 'newrepo' master_bookmark
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a'
  
 

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
  o  6: 2bea0b154d91 public 'new feature on top of master'
  |
  | @  5: f9e4cd522499 draft 'new feature on top of master'
  | |
  | | o  4: eca836c7c651 draft 'change on top of the release'
  | | |
  | | o  3: 500658c138a4 public 'feature release'
  | |/
  o |  2: 95cad53aab1b public 'newrepo' newbook
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

  $ hgmn up 2bea0b154d91 -q
  $ mkcommit ww
  $ hgmn push -r . --to "release 1"  --create -q
  $ mkcommit xx
  $ mkcommit yy
  $ mkcommit zz

  $ hgmn up 2bea0b154d91 -q
  $ mkcommit www
  $ mkcommit xxx
  $ hgmn push -r . --to "release 2"  --create -q
  $ mkcommit yyy
  $ mkcommit zzz

  $ hgmn up 2bea0b154d91 -q
  $ mkcommit wwww
  $ mkcommit xxxx
  $ mkcommit yyyy
  $ hgmn push -r . --to "release 3"  --create -q
  $ mkcommit zzzz

  $ hgmn up 2bea0b154d91 -q
  $ mkcommit wwwww
  $ mkcommit xxxxx
  $ mkcommit yyyyy
  $ mkcommit zzzzz
  $ hgmn push -r . --to "release 4"  --create -q

  $ hgmn cloud backup --dest ssh://user@dummy/repo -q

  $ hgmn cloud check --dest ssh://user@dummy/repo -r e4ae5d869b06 --remote
  e4ae5d869b061c927cf739de04162653c9c4a9ab backed up
  $ hgmn cloud check --dest ssh://user@dummy/repo -r 8336071380f0 --remote
  8336071380f0f7cf919e51f7f1818f2fed425fdb backed up
  $ hgmn cloud check --dest ssh://user@dummy/repo -r 884e1b02454b --remote
  884e1b02454b186570b35d47eac3794b2f8d952f backed up

  $ tglogp
  @  22: 1e6a6d6a35a6 public 'zzzzz'
  |
  o  21: 9279bca0443c public 'yyyyy'
  |
  o  20: 9d588b656b51 public 'xxxxx'
  |
  o  19: 4c250e6f06f5 public 'wwwww'
  |
  | o  18: e4ae5d869b06 draft 'zzzz'
  | |
  | o  17: 3370986ca01b public 'yyyy'
  | |
  | o  16: fb3ba87e275a public 'xxxx'
  | |
  | o  15: 86268dc9ba00 public 'wwww'
  |/
  | o  14: 8336071380f0 draft 'zzz'
  | |
  | o  13: a9e7b029fb67 draft 'yyy'
  | |
  | o  12: 49c7e143a5d1 public 'xxx'
  | |
  | o  11: eedfd85fd934 public 'www'
  |/
  | o  10: 884e1b02454b draft 'zz'
  | |
  | o  9: 43bb408f4831 draft 'yy'
  | |
  | o  8: 8417b8ce7989 draft 'xx'
  | |
  | o  7: bce1d74baf69 public 'ww'
  |/
  o  6: 2bea0b154d91 public 'new feature on top of master'
  |
  | o  3: 500658c138a4 public 'feature release'
  | |
  o |  2: 95cad53aab1b public 'newrepo' newbook
  | |
  o |  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a' master_bookmark
  
At the moment short hahses are not working, print the full hashes to use then in hg pull command
  $ hg log -T '{node}\n' -r 'heads(all())'
  500658c138a447f7ba651547d45ac510ae4e6db2
  884e1b02454b186570b35d47eac3794b2f8d952f
  8336071380f0f7cf919e51f7f1818f2fed425fdb
  e4ae5d869b061c927cf739de04162653c9c4a9ab
  1e6a6d6a35a683a946e96ab16d011883f0dc8f77

  $ cd ../repo-pull

  $ hgmn cloud restorebackup
  abort: 'listkeyspatterns' command is not supported for the server ssh://user@dummy/repo
  [255]

  $ hgmn pull -r 884e1b02454b186570b35d47eac3794b2f8d952f -r 8336071380f0f7cf919e51f7f1818f2fed425fdb -r e4ae5d869b061c927cf739de04162653c9c4a9ab -r 1e6a6d6a35a683a946e96ab16d011883f0dc8f77 -q

  $ tglogpnr -r "::1e6a6d6a35a683a946e96ab16d011883f0dc8f77 - ::master_bookmark"
  o  1e6a6d6a35a6 public 'zzzzz' release 4
  |
  o  9279bca0443c public 'yyyyy'
  |
  o  9d588b656b51 public 'xxxxx'
  |
  o  4c250e6f06f5 public 'wwwww'
  |
  ~
  $ tglogpnr -r "::e4ae5d869b061c927cf739de04162653c9c4a9ab - ::master_bookmark"
  o  e4ae5d869b06 draft 'zzzz'
  |
  o  3370986ca01b public 'yyyy' release 3
  |
  o  fb3ba87e275a public 'xxxx'
  |
  o  86268dc9ba00 public 'wwww'
  |
  ~
  $ tglogpnr -r "::8336071380f0f7cf919e51f7f1818f2fed425fdb - ::master_bookmark"
  o  8336071380f0 draft 'zzz'
  |
  o  a9e7b029fb67 draft 'yyy'
  |
  o  49c7e143a5d1 public 'xxx' release 2
  |
  o  eedfd85fd934 public 'www'
  |
  ~
  $ tglogpnr -r "::884e1b02454b186570b35d47eac3794b2f8d952f - ::master_bookmark"
  o  884e1b02454b draft 'zz'
  |
  o  43bb408f4831 draft 'yy'
  |
  o  8417b8ce7989 draft 'xx'
  |
  o  bce1d74baf69 public 'ww' release 1
  |
  ~
