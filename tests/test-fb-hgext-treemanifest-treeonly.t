TODO: Make this test compatibile with obsstore enabled.
  $ setconfig experimental.evolution=
  $ setconfig treemanifest.flatcompat=False
  $ . "$TESTDIR/library.sh"

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=
  > [treemanifest]
  > server=True
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > EOF

Make local commits on the server
  $ mkdir subdir
  $ echo x > subdir/x
  $ hg commit -qAm 'add subdir/x'

The following will simulate the transition from flat to tree-only
1. Flat only client, with flat only draft commits
2. Hybrid client, with some flat and some flat+tree draft commits
3. Tree-only client, with only tree commits (old flat are converted)

Create flat manifest client
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client -q
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > amend=
  > pushrebase=
  > EOF

Make a flat-only draft commit
  $ echo f >> subdir/x
  $ hg commit -qm "flat only commit"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Transition to hybrid flat+tree client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > demanddownload=True
  > EOF

# Viewing commit from server should download trees
  $ hg log -r . --stat -T "{desc}\n"
  add subdir/x
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ ls_l $CACHEDIR/master/packs/manifests | wc -l
  \s*4 (re)

# Viewing flat draft commit should not produce tree packs
  $ hg log -r tip --stat -T '{desc}\n'
  flat only commit
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ ls_l $CACHEDIR/master/packs/manifests | wc -l
  \s*4 (re)

Make a local hybrid flat+tree draft commit
  $ echo h >> subdir/x
  $ ls_l .hg/store/packs | grep manifests
  drwxrwxr-x         manifests
  $ hg commit -qm "hybrid flat+tree commit"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1196 3d24c0f5de7320d93b08453373aedf295e433c09.histidx
  -r--r--r--     183 3d24c0f5de7320d93b08453373aedf295e433c09.histpack
  -r--r--r--    1114 48dcae2c46611b2159b72c9dec819b95cfcf3df7.dataidx
  -r--r--r--     219 48dcae2c46611b2159b72c9dec819b95cfcf3df7.datapack
  -r--r--r--    1196 73638ba2b0129f1d74a7b455dab952c6634e52c8.histidx
  -r--r--r--     183 73638ba2b0129f1d74a7b455dab952c6634e52c8.histpack
  -r--r--r--    1114 764ae1eb8d6408a3271d5c5f4fc94b2d2e1d3002.dataidx
  -r--r--r--     219 764ae1eb8d6408a3271d5c5f4fc94b2d2e1d3002.datapack

Enable sendtrees and verify flat is converted to tree on demand
  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF
  $ hg log -r 1 --stat
  changeset:   1:f3216a7f98b5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     flat only commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)
  $ hg repack

Make a local tree-only draft commit
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*4 (re)
  $ ls .hg/store/packs/manifests > $TESTTMP/origpacks
  $ echo t >> subdir/x
  $ hg commit -qm "tree only commit"
  $ ls .hg/store/packs/manifests > $TESTTMP/aftercommit1packs
  $ hg debugdata -c 3
  0b096b20288404c17aa355fdeca48decf58d745d
  test
  0 0
  subdir/x
  
  tree only commit (no-eol)
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)
# No manifest revlog revision was added
  $ hg debugindex -m --config treemanifest.treeonly=False
  hg debugindex: invalid arguments
  (use 'hg debugindex -h' to get help)
  [255]

Tree-only amend
  $ echo >> subdir/x
  $ hg commit --amend
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/43903a6bf43f-712cb952-amend.hg (glob)
# amend commit was added
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*12 (re)
# No manifest revlog revision was added
  $ hg debugindex -m --config treemanifest.treeonly=False
  hg debugindex: invalid arguments
  (use 'hg debugindex -h' to get help)
  [255]

# Delete the original commits packs
  $ cd .hg/store/packs/manifests
  $ rm -rf `comm -3 $TESTTMP/origpacks $TESTTMP/aftercommit1packs`
  $ cd ../../../..
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)

Test looking at the tree from inside the bundle
  $ hg log -r tip -vp -R $TESTTMP/client/.hg/strip-backup/43903a6bf43f-712cb952-amend.hg --pager=off
  changeset:   4:43903a6bf43f
  tag:         tip
  parent:      0:d618f764f9a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  tree only commit
  
  
  diff -r d618f764f9a1 -r 43903a6bf43f subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   x
  +t
  

Test unbundling the original commit
# Bring the original commit back from the bundle
  $ hg unbundle $TESTTMP/client/.hg/strip-backup/43903a6bf43f-712cb952-amend.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 43903a6bf43f
# Verify the packs were brought back and the data is accessible
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*12 (re)
  $ hg log -r tip --stat
  changeset:   4:43903a6bf43f
  tag:         tip
  parent:      0:d618f764f9a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     tree only commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
Test pulling new commits from a hybrid server
  $ cd ../master
  $ echo x >> subdir/x
  $ hg commit -qAm 'modify subdir/x'
  $ hg log -r tip -T '{manifest}'
  7e265a5dc5229c2b237874c6bd19f6ef4120f949 (no-eol)

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets 098a163f13ea

  $ hg debugindex -m --config treemanifest.treeonly=False
  hg debugindex: invalid arguments
  (use 'hg debugindex -h' to get help)
  [255]
  $ hg log -r tip --stat --pager=off
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949, based on 5fbe397e5ac6cb7ee263c5c67613c4665306d143* (glob)
  2 trees fetched over * (glob)
  changeset:   5:098a163f13ea
  tag:         tip
  parent:      0:d618f764f9a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify subdir/x
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Test rebasing treeonly commits
  $ hg rebase -d 5 -b 2
  rebasing abc828a8166c "hybrid flat+tree commit"
  merging subdir/x
  warning: 1 conflicts while merging subdir/x! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ printf "x\nx\nh\n" > subdir/x
  $ hg resolve --mark subdir/x
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing abc828a8166c "hybrid flat+tree commit"
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/abc828a8166c-26189234-rebase.hg (glob)

Test histedit treeonly commits
  $ hg up -q bb62fe710976
  $ hg purge --config extensions.purge=
  $ echo y > y
  $ hg commit -Aqm 'add y'
  $ hg histedit --config extensions.histedit= --commands - <<EOF
  > pick 3d62f4200ab6 add y
  > pick bb62fe710976 hybrid flat+tree commit
  > EOF
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/bb62fe710976-52ac100d-histedit.hg (glob)
  $ hg log -l 2 -G -T '{desc}'
  @  hybrid flat+tree commit
  |
  o  add y
  |
  ~

Test {manifest} template
  $ hg log -r . -T '{manifest}'
  5f15f80c2b54c16d75780bd0344a0487d4e6ff3b (no-eol)

Test turning treeonly off and making sure we can still commit on top of treeonly
commits
  $ echo >> subdir/x
  $ hg debugindex -m --config treemanifest.treeonly=False | tail -1
  hg debugindex: invalid arguments
  (use 'hg debugindex -h' to get help)
  $ hg commit -m 'treeonly from hybrid repo'
  $ hg log -r . -T '{desc}\n' --stat
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg log -r . -T '{desc}\n' --stat
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg debugindex -m --config treemanifest.treeonly=False | tail -1
  hg debugindex: invalid arguments
  (use 'hg debugindex -h' to get help)
  $ hg debugstrip -r .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/41373853bc69-c732668d-backup.hg

Test peer-to-peer push/pull of tree only commits
  $ cd ..
  $ clearcache
  $ hgcloneshallow ssh://user@dummy/master client2 -q
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949, found via 098a163f13ea
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client2
  $ rm -rf $CACHEDIR
  $ cp ../client/.hg/hgrc .hg/hgrc

# Test pulling from a treeonly peer
# - We should see one tree recieve from the client, and then a second one when
#   prefetching the draft commit parent.
  $ hg pull -r tip ssh://user@dummy/client --debug 2>&1 | egrep "(payload|treegroup|running)"
  running python "*" 'user@dummy' 'hg -R client serve --stdio' (glob)
  running python "*" 'user@dummy' 'hg -R master serve --stdio' (glob)
  bundle2-input-part: total payload size 827
  bundle2-input-part: total payload size 48
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size 663
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size 388
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -l 3 -T '{desc}\n' --stat
  @  hybrid flat+tree commit
  |   subdir/x |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  add y
  |   y |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143, based on 7e265a5dc5229c2b237874c6bd19f6ef4120f949
  2 trees fetched over * (glob)
  o  modify subdir/x
  |   subdir/x |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

# Test pushing to a treeonly peer
  $ echo y >> y
  $ hg commit -qm "modify y"
  $ hg push -f -r . ssh://user@dummy/client --debug 2>&1 | grep treegroup
  bundle2-output-part: "b2x:treegroup2" (params: 3 mandatory) streamed payload
  $ cd ../client
  $ hg log -r tip -T '{desc}\n' --stat
  modify y
   y |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
Test bundling
  $ hg bundle -r 'tip~3::tip' ../mybundle.hg
  searching for changes
  3 changesets found
  $ cd ..
  $ hgcloneshallow ssh://user@dummy/master client3 -q
  $ cd client3
  $ cp ../client/.hg/hgrc .hg/hgrc
  $ hg unbundle ../mybundle.hg
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  new changesets 41bd8aa2aeb7:06f5aa20a0d4
  $ hg log -r 'tip^::tip' -G -T "{desc}\n" --stat
  o  modify y
  |   y |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  hybrid flat+tree commit
  |   subdir/x |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  
Test pushing to a hybrid server w/ pushrebase w/ hooks
  $ cat >> $TESTTMP/filehook.sh <<EOF
  > #!/bin/bash
  > set -xe
  > [[ \$(hg log -r \$HG_NODE -T '{file_adds}') == 'y' ]] && exit 1
  > echo \$(hg log -r \$HG_NODE -T '{file_adds}')
  > exit 2
  > EOF
  $ chmod a+x $TESTTMP/filehook.sh
  $ cat >> ../master/.hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.fail=$TESTTMP/filehook.sh
  > EOF
  $ hg push -r 2 --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: ++ hg log -r 41bd8aa2aeb72ff43d5bc329fa115c7c8f9c6f7f -T '{file_adds}'
  remote: + [[ y == \y ]]
  remote: + exit 1
  remote: prepushrebase.fail hook exited with status 1
  abort: push failed on remote
  [255]

Test pushing to a hybrid server w/ pushrebase w/o hooks
  $ cd ../master
  $ cat >> .hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.fail=true
  > EOF
- Add an extra head to the master repo so we trigger the slowpath
- shallowbundle.generatemanifests() codepath, so we can verify it doesnt try to
- process all the manifests either.
  $ hg up -q 0
  $ echo >> extrahead
  $ hg commit -Aqm 'extra head commit'
  $ hg up -q 1
  $ cd ../client3

  $ hg push -r 2 --to master --debug 2>&1 | egrep '(remote:|add|converting)'
  remote: * (glob)
  remote: * (glob)
  remote: 1
  remote: pushing 1 changeset:
  remote:     41bd8aa2aeb7  add y

  $ cd ../master
- Delete the temporary commit we made earlier
  $ hg debugstrip -qr 3

- Verify the received tree was written down as a flat
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 5fbe397e5ac6 000000000000 000000000000
       1        50      50     -1       1 7e265a5dc522 5fbe397e5ac6 000000000000
       2       100      62      0       2 9bd1ef658bef 5fbe397e5ac6 000000000000
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 5fbe397e5ac6 000000000000 000000000000
       1        50      50     -1       1 7e265a5dc522 5fbe397e5ac6 000000000000
       2       100      62      0       2 9bd1ef658bef 5fbe397e5ac6 000000000000
- Verify the manifest data is accessible
  $ hg log -r tip --stat
  changeset:   2:dad1be784127
  tag:         tip
  parent:      0:d618f764f9a1
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     extra head commit
  
   extrahead |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Test prefetch
  $ cd ../client
  $ clearcache
  $ hg prefetch -r 0
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ clearcache

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files
  new changesets dad1be784127
  $ cd ..

Test converting server to treeonly
  $ cd master
  $ cp .hg/hgrc .hg/hgrc.bak
- Move the flat manifest away so we guarantee its not read
  $ hg log -G -T "{desc}\n" --stat
  o  extra head commit
  |   extrahead |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  | @  modify subdir/x
  |/    subdir/x |  1 +
  |     1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  add subdir/x
      subdir/x |  1 +
      1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg up -q tip
  $ echo >> subdir/x
  $ hg commit -m 'modify subdir/x again'
  $ hg log -r tip -T "{desc}\n" --stat
  modify subdir/x again
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
Test pulling to a treeonly client from a treeonly server
  $ cd ../client2
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files
  new changesets dad1be784127:7253109af085
  $ hg log -r tip -T '{desc}\n' --stat
  fetching tree '' 9bd1ef658bef2ded12bd295198d1abbe1cf4115b, found via dad1be784127
  2 trees fetched over * (glob)
  fetching tree '' e249b5cd4abe985a0e7ecd0af4d66d60e560ef4c, based on 9bd1ef658bef2ded12bd295198d1abbe1cf4115b, found via 7253109af085
  2 trees fetched over * (glob)
  modify subdir/x again
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)

Test pushing from a treeonly client to a treeonly server
  $ hg config treemanifest
  treemanifest.flatcompat=False
  treemanifest.rustmanifest=True
  treemanifest.sendtrees=True
  treemanifest.demanddownload=True
  $ echo 'pushable' >> subdir/x
  $ hg commit -Aqm 'pushable treeonly commit'

Test pushing from a treeonly client to a treeonly server *without* pushrebase

  $ hg log -Gf -l 4 -T '{shortest(node)} {manifest|short}\n'
  @  5f0b c760d8ba4646
  |
  o  06f5 13c9facfa409
  |
  o  77ec 5f15f80c2b54
  |
  o  41bd 14bce01d0d73
  |
  ~
  $ hg push -r . --config extensions.pushrebase=! -f
  pushing to ssh://user@dummy/master
  searching for changes
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b (?)
  1 trees fetched over * (glob) (?)
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949, found via 5f0bc1aaff22
  2 trees fetched over * (glob)
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 4 changesets with 4 changes to 2 files
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ hg --cwd ../master debugindex .hg/store/00manifesttree.i | tail -4
       4       223      55      1       4 14bce01d0d73 7e265a5dc522 000000000000
       5       278      61      4       5 5f15f80c2b54 14bce01d0d73 000000000000
       6       339      80     -1       6 13c9facfa409 5f15f80c2b54 000000000000
       7       419      60      6       7 c760d8ba4646 13c9facfa409 000000000000
  $ hg --cwd ../master debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      44     -1       1 a18d21674e76 bc0c2c938b92 000000000000
       2        88      44     -1       3 986e3ffada22 bc0c2c938b92 000000000000
       3       132      44     -1       5 f4c373af9a41 a18d21674e76 000000000000
       4       176      44     -1       7 d20854ad7783 f4c373af9a41 000000000000
  $ hg -R ../master log -r tip --stat
  changeset:   7:5f0bc1aaff22
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushable treeonly commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg -R ../master debugstrip -q -r tip~3
  $ hg phase -dfr tip~3

Test pushing from a public treeonly client to a treeonly server *with* pushrebase
(it should send tree data, versus p2p pushes which wouldnt)
  $ hg phase -pr tip~2
  $ hg push -r . --config extensions.pushrebase=! -f
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 4 changesets with 4 changes to 2 files
  $ hg -R ../master log -r tip --stat
  changeset:   7:5f0bc1aaff22
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushable treeonly commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg -R ../master debugstrip -r tip~3
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/41bd8aa2aeb7-4ba57e03-backup.hg
  $ hg phase -dfr tip~3

Test pushing from a treeonly client to a treeonly server *with* pushrebase

  $ hg push --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 4 changesets:
  remote:     41bd8aa2aeb7  add y
  remote:     77ec7ac93315  hybrid flat+tree commit
  remote:     06f5aa20a0d4  modify y
  remote:     5f0bc1aaff22  pushable treeonly commit
  $ hg -R ../master log -l 4 -T '{desc}\n' -G --stat
  o  pushable treeonly commit
  |   subdir/x |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  modify y
  |   y |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  hybrid flat+tree commit
  |   subdir/x |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  o  add y
  |   y |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  
Strip the pushed commits + the recently made commit from the server
  $ hg -R ../master debugstrip -r '.:'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/7253109af085-5cce35c9-backup.hg

Reset the phase of the local commits to draft
  $ hg phase -fd 2::

Test histedit with changing commits in the middle
  $ cat >> $TESTTMP/commands <<EOF
  > pick 06f5aa20a0d4 4
  > x echo >> y && hg amend
  > pick 5f0bc1aaff22 7
  > EOF
  $ hg histedit '.^' --commands $TESTTMP/commands --config extensions.histedit= --config extensions.fbhistedit=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: orphaned descendants detected, not stripping 06f5aa20a0d4
  saved backup bundle to $TESTTMP/client2/.hg/strip-backup/5f0bc1aaff22-2d1d80a0-histedit.hg

Reset the server back to hybrid mode
  $ cd ../master
  $ mv .hg/hgrc.bak .hg/hgrc
  $ cd ..

Test creating a treeonly repo from scratch
  $ hg init treeonlyrepo
  $ cd treeonlyrepo
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > sendtrees=True
  > 
  > [remotefilelog]
  > reponame=treeonlyrepo
  > EOF
  $ echo foo > a
  $ hg commit -Aqm 'add a'
  $ hg log -r . -p
  changeset:   0:f87d03aef498
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add a
  
  diff -r 000000000000 -r f87d03aef498 a
  --- /dev/null	Thu Jan 01 00:00:00 1970 +0000
  +++ b/a	Thu Jan 01 00:00:00 1970 +0000
  @@ -0,0 +1,1 @@
  +foo
  
Test pulling new commits from a local repository (not over ssh),
with pullprefetchrevs configured.
  $ cd ..
  $ hg init treeonlyrepo2
  $ cd treeonlyrepo2
  $ cat >> .hg/hgrc <<EOF
  > [paths]
  > default=$TESTTMP/master
  > 
  > [treemanifest]
  > sendtrees=True
  > pullprefetchrevs=tip
  > 
  > [remotefilelog]
  > reponame=treeonlyrepo2
  > EOF
  $ hg pull
  pulling from $TESTTMP/master
  requesting all changes
  adding changesets
  adding manifests
  adding file changes
  added 3 changesets with 3 changes to 2 files
  new changesets d618f764f9a1:dad1be784127
  prefetching tree for dad1be784127
  2 trees fetched over * (glob)
  $ cd ..

Test ondemand downloading trees with a limited depth
  $ hgcloneshallow ssh://user@dummy/master client4 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client4
  $ cp ../client/.hg/hgrc .hg/hgrc

  $ clearcache
  $ hg status --change 'tip^'
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  2 trees fetched over * (glob)
  A subdir/x

  $ clearcache
  $ hg status --change 'tip^' --config treemanifest.fetchdepth=1
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 trees fetched over * (glob)
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  A subdir/x
