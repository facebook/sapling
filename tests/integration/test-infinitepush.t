  $ . $TESTDIR/library.sh

setup configuration
  $ setup_common_config
  $ cd $TESTTMP

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
  > infinitepush=
  > infinitepushbackup=
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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey stream-preferred stream_option streamreqs=generaldelta,lz4revlog,revlogv1 treeonly bundle2=HG20%0Achangegroup%3D02%0Ab2x%3Ainfinitepush%0Ab2x%3Ainfinitepushscratchbookmarks%0Apushkey%0Atreemanifestserver%3DTrue%0Ab2x%3Arebase%0Ab2x%3Arebasepackpart%0Aphases%3Dheads
  remote: 1
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
  bundle2-output-part: "replycaps" 283 bytes payload
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
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
  sending branchmap command
  $ tglogp
  @  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ cd ../repo-pull
  $ hgmn pull -r 47da8b81097c5534f3eb7947a8764dd323cffe3d
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  no changes found
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 47da8b81097c
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
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
  $ hgmn pushbackup ssh://user@dummy/repo --debug
  starting backup* (glob)
  backing up stack rooted at 47da8b81097c
  running * (glob)
  sending hello command
  sending between command
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey stream-preferred stream_option streamreqs=generaldelta,lz4revlog,revlogv1 treeonly bundle2=HG20%0Achangegroup%3D02%0Ab2x%3Ainfinitepush%0Ab2x%3Ainfinitepushscratchbookmarks%0Apushkey%0Atreemanifestserver%3DTrue%0Ab2x%3Arebase%0Ab2x%3Arebasepackpart%0Aphases%3Dheads
  remote: 1
  2 changesets found
  list of changesets:
  47da8b81097c5534f3eb7947a8764dd323cffe3d
  95cad53aab1b0b33eceee14473b3983312721529
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 3 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSH" (params: 0 advisory) streamed payload
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  backup complete
  heads added: 95cad53aab1b0b33eceee14473b3983312721529
  heads removed:  (re)
  finished in * seconds (glob)

  $ tglogp
  @  2: 95cad53aab1b draft 'newrepo'
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

  $ cd ../repo-pull
  $ hgmn pull -r 95cad53aab1b0b33eceee14473b3983312721529
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  no changes found
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
  $ hgmn pushbackup ssh://user@dummy/repo --debug
  starting backup* (glob)
  running * (glob)
  sending hello command
  sending between command
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  remote: * (glob)
  remote: capabilities: lookup known getbundle unbundle=HG10GZ,HG10BZ,HG10UN gettreepack remotefilelog pushkey stream-preferred stream_option streamreqs=generaldelta,lz4revlog,revlogv1 treeonly bundle2=HG20%0Achangegroup%3D02%0Ab2x%3Ainfinitepush%0Ab2x%3Ainfinitepushscratchbookmarks%0Apushkey%0Atreemanifestserver%3DTrue%0Ab2x%3Arebase%0Ab2x%3Arebasepackpart%0Aphases%3Dheads
  remote: 1
  sending unbundle command
  bundle2-output-bundle: "HG20", (1 params) 2 parts total
  bundle2-output-part: "replycaps" * bytes payload (glob)
  bundle2-output-part: "B2X:INFINITEPUSHSCRATCHBOOKMARKS" * bytes payload (glob)
  backup complete
  heads added:  (re)
  heads removed:  (re)
  finished in * seconds (glob)

  $ tglogp
  @  2: 95cad53aab1b draft 'newrepo' newbook
  |
  o  1: 47da8b81097c draft 'new'
  |
  o  0: 3903775176ed public 'a' master_bookmark
  

Finally, try to push existing commit to a public bookmark
  $ hgmn push -r . --to master_bookmark
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev 500658c138a4 to destination ssh://user@dummy/repo bookmark test_release_1.0.0
  searching for changes
  exporting bookmark test_release_1.0.0
  $ echo new > file2
  $ hg addremove -q
  $ hg ci -m "change on top of the release"
  $ hgmn pushbackup ssh://user@dummy/repo
  starting backup* (glob)
  backing up stack rooted at eca836c7c651
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  finished in * seconds (glob)

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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files (+1 heads)
  adding remote bookmark test_release_1.0.0
  new changesets 500658c138a4:eca836c7c651
  (run 'hg heads' to see heads, 'hg merge' to merge)

# Note: For this case phases are not returned correctly (see TODO in implementation)
# phase for test_release_1.0.0 is incorrect
  $ tglogp
  o  4: eca836c7c651 draft 'change on top of the release'
  |
  o  3: 500658c138a4 draft 'feature release' test_release_1.0.0
  |
  | @  2: 95cad53aab1b public 'newrepo' master_bookmark
  | |
  | o  1: 47da8b81097c public 'new'
  |/
  o  0: 3903775176ed public 'a'
  

  $ hgmn pull -r test_release_1.0.0
  pulling from ssh://user@dummy/repo
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
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
  remote: * DEBG Session with Mononoke started with uuid: * (glob)
  pushing rev f9e4cd522499 to destination ssh://user@dummy/repo bookmark master_bookmark
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  server ignored bookmark master_bookmark update

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
  
