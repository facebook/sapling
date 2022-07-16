#chg-compatible
  $ setconfig experimental.allowfilepeer=True
  $ setconfig config.use-rust=true

  $ . "$TESTDIR/library.sh"

Setup the server

  $ hginit master
  $ cd master
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
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
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
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
  
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

# Viewing flat draft commit should not produce tree packs
  $ hg log -r tip --stat -T '{desc}\n'
  flat only commit
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Make a local hybrid flat+tree draft commit
  $ echo h >> subdir/x
  $ ls_l .hg/store/packs | grep manifests
  drwxrwxr-x         manifests
  $ hg commit -qm "hybrid flat+tree commit"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Enable sendtrees and verify flat is converted to tree on demand
  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF
  $ hg log -r f3216a7f98b5a80b45db6bd600d958cbffa49d9e --stat
  commit:      f3216a7f98b5
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     flat only commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Make a local tree-only draft commit
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
  $ echo t >> subdir/x
  $ hg commit -qm "tree only commit"
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
  $ hg debugdata -c 3
  0b096b20288404c17aa355fdeca48decf58d745d
  test
  0 0
  subdir/x
  
  tree only commit (no-eol)
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

Tree-only amend
  $ echo >> subdir/x
  $ hg commit --amend
# amend commit was added
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.
# No manifest revlog revision was added
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

  $ hg log -r 'predecessors(tip)-tip' --stat
  commit:      43903a6bf43f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     tree only commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
TODO(meyer): Fix debugindexedlogdatastore and debugindexedloghistorystore and add back output here.

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

  $ hg log -r tip --stat --pager=off
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949
  1 trees fetched over 0.00s
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b
  1 trees fetched over 0.00s
  commit:      098a163f13ea
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify subdir/x
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Test rebasing treeonly commits
  $ hg rebase -d 'max(desc(modify))' -b 'max(desc(hybrid))'
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

Test histedit treeonly commits
  $ hg up -q bb62fe710976
  $ hg purge
  $ echo y > y
  $ hg commit -Aqm 'add y'
  $ hg histedit bb62fe710976 --config extensions.histedit= --commands - <<EOF
  > pick 3d62f4200ab6 add y
  > pick bb62fe710976 hybrid flat+tree commit
  > EOF
  $ hg log -l 2 -G -T '{desc}'
  @  hybrid flat+tree commit
  │
  o  add y
  │
  ~

Test {manifest} template
  $ hg log -r . -T '{manifest}'
  5f15f80c2b54c16d75780bd0344a0487d4e6ff3b (no-eol)

Test turning treeonly off and making sure we can still commit on top of treeonly
commits
  $ echo >> subdir/x
  $ hg commit -m 'treeonly from hybrid repo'
  $ hg log -r . -T '{desc}\n' --stat
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg log -r . -T '{desc}\n' --stat
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg debugstrip -r .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

Test peer-to-peer push/pull of tree only commits
  $ cd ..
  $ clearcache
  $ hgcloneshallow ssh://user@dummy/master client2 -q
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b
  1 trees fetched over 0.00s
  $ cd client2
  $ rm -rf $CACHEDIR
  $ cp ../client/.hg/hgrc .hg/hgrc

# Test pulling from a treeonly peer
# - We should see one tree recieve from the client, and then a second one when
#   prefetching the draft commit parent.
  $ hg pull -r tip ssh://user@dummy/client --debug 2>&1 | egrep "(payload|treegroup|running)"
  running * 'user@dummy' 'hg -R client serve --stdio' (glob)
  running * 'user@dummy' 'hg -R master serve --stdio' (glob)
  bundle2-input-part: total payload size 831
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size 663
  $ hg up tip
  2 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg log -G -l 3 -T '{desc}\n' --stat
  @  hybrid flat+tree commit
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  │   subdir/x |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  add y
  │   y |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
  o  modify subdir/x
  │   subdir/x |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  

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
  $ hg log -r 'tip^::tip' -G -T "{desc}\n" --stat
  o  modify y
  │   y |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  hybrid flat+tree commit
  │   subdir/x |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  
Test pushing to a hybrid server w/ pushrebase w/ hooks
  $ cat >> $TESTTMP/filehook.sh <<EOF
  > #!/bin/bash
  > set -e
  > [[ \$(hg log -r \$HG_NODE -T '{file_adds}') == 'y' ]] && exit 1
  > echo \$(hg log -r \$HG_NODE -T '{file_adds}')
  > exit 2
  > EOF
  $ chmod a+x $TESTTMP/filehook.sh
  $ cat >> ../master/.hg/hgrc <<EOF
  > [hooks]
  > prepushrebase.fail=$TESTTMP/filehook.sh
  > EOF
  $ hg push -r 'max(desc(add))' --to master
  pushing to ssh://user@dummy/master
  searching for changes
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
  $ hg up -q 'desc(add)'
  $ echo >> extrahead
  $ hg commit -Aqm 'extra head commit'
  $ hg up -q 'desc(modify)'
  $ cd ../client3

  $ hg push -r 'max(desc(add))' --to master --debug 2>&1 | egrep '(remote:|add|converting)'
  remote: * (glob)
  remote: * (glob)
  remote: 1
  remote: pushing 1 changeset:
  remote:     41bd8aa2aeb7  add y

  $ cd ../master
- Delete the temporary commit we made earlier
  $ hg debugstrip -qr 'max(desc(add))'

- Verify the received tree was written down as a flat
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 5fbe397e5ac6 000000000000 000000000000
       1        50      50     -1       1 7e265a5dc522 5fbe397e5ac6 000000000000
       2       100      63      0       2 9bd1ef658bef 5fbe397e5ac6 000000000000
       3       163      55      1       3 14bce01d0d73 7e265a5dc522 000000000000
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 5fbe397e5ac6 000000000000 000000000000
       1        50      50     -1       1 7e265a5dc522 5fbe397e5ac6 000000000000
       2       100      63      0       2 9bd1ef658bef 5fbe397e5ac6 000000000000
       3       163      55      1       3 14bce01d0d73 7e265a5dc522 000000000000
- Verify the manifest data is accessible
  $ hg log -r tip --stat
  commit:      dad1be784127
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     extra head commit
  
   extrahead |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Test prefetch
  $ cd ../client
  $ clearcache
  $ hg prefetch -r d618f764f9a11819b57268f02604ec1d311afc4c
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
  $ clearcache

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  $ cd ..

Test converting server to treeonly
  $ cd master
  $ cp .hg/hgrc .hg/hgrc.bak
- Move the flat manifest away so we guarantee its not read
  $ hg log -G -T "{desc}\n" --stat
  o  extra head commit
  │   extrahead |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  │ @  modify subdir/x
  ├─╯   subdir/x |  1 +
  │     1 files changed, 1 insertions(+), 0 deletions(-)
  │
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
  $ hg log -r tip -T '{desc}\n' --stat
  fetching tree '' 9bd1ef658bef2ded12bd295198d1abbe1cf4115b
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree '' e249b5cd4abe985a0e7ecd0af4d66d60e560ef4c
  1 trees fetched over 0.00s
  2 trees fetched over 0.00s
  modify subdir/x again
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Test pushing from a treeonly client to a treeonly server
  $ hg config treemanifest
  treemanifest.demanddownload=True
  treemanifest.rustmanifest=True
  treemanifest.sendtrees=True
  treemanifest.treeonly=True
  treemanifest.useruststore=True
  $ echo 'pushable' >> subdir/x
  $ hg commit -Aqm 'pushable treeonly commit'

Test pushing from a treeonly client to a treeonly server *without* pushrebase

  $ hg log -Gf -l 4 -T '{shortest(node)} {manifest|short} {remotenames} {bookmarks}\n'
  @  5f0b c760d8ba4646
  │
  o  06f5 13c9facfa409
  │
  o  77ec 5f15f80c2b54
  │
  o  41bd 14bce01d0d73
  │
  ~
  $ hg push -r . --config extensions.pushrebase=! -f
  pushing to ssh://user@dummy/master
  searching for changes
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b (?)
  1 trees fetched over * (glob) (?)
  fetching tree '' 7e265a5dc5229c2b237874c6bd19f6ef4120f949
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b
  1 trees fetched over 0.00s
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ hg --cwd ../master debugindex .hg/store/00manifesttree.i | tail -4
       4       218      61      2       3 e249b5cd4abe 9bd1ef658bef 000000000000
       5       279      61      3       5 5f15f80c2b54 14bce01d0d73 000000000000
       6       340      93     -1       6 13c9facfa409 5f15f80c2b54 000000000000
       7       433      61      6       7 c760d8ba4646 13c9facfa409 000000000000
  $ hg --cwd ../master debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      44     -1       1 a18d21674e76 bc0c2c938b92 000000000000
       2        88      44     -1       3 986e3ffada22 bc0c2c938b92 000000000000
       3       132      44     -1       5 f4c373af9a41 a18d21674e76 000000000000
       4       176      44     -1       7 d20854ad7783 f4c373af9a41 000000000000
  $ hg -R ../master log -r tip --stat
  commit:      5f0bc1aaff22
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushable treeonly commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg -R ../master debugstrip -q -r tip~3

Test pushing from a public treeonly client to a treeonly server *with* pushrebase
(it should send tree data, versus p2p pushes which wouldnt)
  $ hg log -T '{desc} {remotenames} {bookmarks}\n' -G
  @  pushable treeonly commit
  │
  │ o  modify subdir/x again
  │ │
  │ o  extra head commit
  │ │
  o │  modify y
  │ │
  o │  hybrid flat+tree commit
  │ │
  o │  add y
  │ │
  o │  modify subdir/x
  ├─╯
  o  add subdir/x
  
  $ hg debugmakepublic -r .~2
  $ hg push -r . --config extensions.pushrebase=! -f
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  $ hg -R ../master log -r tip --stat
  commit:      5f0bc1aaff22
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushable treeonly commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg debugmakepublic -d -r 'desc(pushable)~2'
  $ hg -R ../master debugstrip -r tip~3

Test pushing from a treeonly client to a treeonly server *with* pushrebase

  $ hg log -T '{desc} {remotenames} {bookmarks}\n' -G
  @  pushable treeonly commit
  │
  │ o  modify subdir/x again
  │ │
  │ o  extra head commit
  │ │
  o │  modify y
  │ │
  o │  hybrid flat+tree commit
  │ │
  o │  add y
  │ │
  o │  modify subdir/x
  ├─╯
  o  add subdir/x
  
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
  │   subdir/x |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  modify y
  │   y |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  hybrid flat+tree commit
  │   subdir/x |  1 +
  │   1 files changed, 1 insertions(+), 0 deletions(-)
  │
  o  add y
  │   y |  1 +
  ~   1 files changed, 1 insertions(+), 0 deletions(-)
  
Strip the pushed commits + the recently made commit from the server
  $ hg -R ../master debugstrip -r '.:'
  1 files updated, 0 files merged, 1 files removed, 0 files unresolved

Test histedit with changing commits in the middle
  $ cat >> $TESTTMP/commands <<EOF
  > pick 06f5aa20a0d4 4
  > x echo >> y && hg amend
  > pick 5f0bc1aaff22 7
  > EOF
  $ hg histedit '.^' --commands $TESTTMP/commands --config extensions.histedit= --config extensions.fbhistedit=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved

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
  commit:      f87d03aef498
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
  > default=ssh://user@dummy/master
  > 
  > [treemanifest]
  > sendtrees=True
  > pullprefetchrevs=tip
  > 
  > [remotefilelog]
  > reponame=treeonlyrepo2
  > EOF
  $ hg pull
  pulling from ssh://user@dummy/master
  streaming all changes
  7 files to transfer, 2.46 KB of data
  transferred 2.46 KB in 0.0 seconds (2.41 MB/sec)
  searching for changes
  no changes found
  prefetching tree for dad1be784127
  fetching tree '' 9bd1ef658bef2ded12bd295198d1abbe1cf4115b
  1 trees fetched over 0.00s
  $ cd ..

Test ondemand downloading trees with a limited depth
  $ hgcloneshallow ssh://user@dummy/master client4 -q
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  $ cd client4
  $ cp ../client/.hg/hgrc .hg/hgrc

  $ clearcache
  $ hg status --change 'tip^'
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 trees fetched over 0.00s
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over 0.00s
  A subdir/x

  $ clearcache
  $ hg status --change 'tip^' --config treemanifest.fetchdepth=1
  fetching tree '' 5fbe397e5ac6cb7ee263c5c67613c4665306d143
  1 trees fetched over * (glob)
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  A subdir/x

  $ cd ..

Make a second repo with some flat manifests and some treeonly manifests, then
push it to the treeonly and verify it can be pushed. This simulates merging an
old repository into another repo.

  $ hginit secondmaster
  $ cd secondmaster
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames=
  > treemanifest=$TESTDIR/../edenscm/hgext/treemanifestserver.py
  > [remotefilelog]
  > server=True
  > shallowtrees=True
  > [treemanifest]
  > treeonly=False
  > sendtrees=False
  > server=True
  > EOF
  $ mkdir second_dir
  $ echo s >> second_dir/s
  $ hg commit -qAm 'flat manifest commit'
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > sendtrees=True
  > EOF
  $ echo s >> second_dir/s
  $ hg commit -qAm 'treeonly manifest commit'
  $ cd ..

  $ hgcloneshallow ssh://user@dummy/secondmaster secondclient -q
  fetching tree '' dcf227dd21f37ac6b3848ab69ee0d0910dbb4071
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob) (?)
  1 trees fetched over 0.00s
  fetching tree 'second_dir' c7e02064396ec447f87d3ca29213b2213c661aa4
  1 trees fetched over 0.00s
  $ cd secondclient
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > pushrebase=
  > remotenames=
  > treemanifest=
  > EOF

  $ hg push --config extensions.pushrebase=! ssh://user@dummy/master -f --to main --create
  pushing rev 0a0cac7a2bb2 to destination ssh://user@dummy/master bookmark main
  searching for changes
  warning: repository is unrelated
  fetching tree '' 99dd81527cb1abd011deb06b629366bfc7c76e3a
  1 trees fetched over 0.00s
  fetching tree 'second_dir' 52db1b54e51c03c6c0d929873d0a725c3ceca286
  1 trees fetched over 0.00s
  exporting bookmark main
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes

  $ hg log -G -T '{node}'
  @  0a0cac7a2bb2ff6613da8280f7f356863cee022b
  │
  o  03e23940cb22c80ad0d5abf1d4dc8f31dec3b945
  
  $ hg -R ../master log -G -T '{node}'
  o  0a0cac7a2bb2ff6613da8280f7f356863cee022b
  │
  o  03e23940cb22c80ad0d5abf1d4dc8f31dec3b945
  
  o  dad1be7841274c8bc9fe4772c99e52833240f715
  │
  │ @  098a163f13ea73eb83a2bd8b426560575e1e91eb
  ├─╯
  o  d618f764f9a11819b57268f02604ec1d311afc4c
  

# trailing whitespace
