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
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > [fastmanifest]
  > usetree=True
  > usecache=False
  > [treemanifest]
  > demanddownload=True
  > EOF

# Viewing commit from server should download trees
  $ hg log -r . --stat -T "{desc}\n"
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
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
  $ ls_l .hg/store | grep packs
  [1]
  $ hg commit -qm "hybrid flat+tree commit"
  $ hg up '.^'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ ls_l .hg/store/packs/manifests
  -r--r--r--    1114 768e50b2051c807d50b545de7cd42db8c0789a53.dataidx
  -r--r--r--     219 768e50b2051c807d50b545de7cd42db8c0789a53.datapack
  -r--r--r--    1196 a5c12ff082e94f0aabc66725c89bcb2e624310bf.histidx
  -r--r--r--     183 a5c12ff082e94f0aabc66725c89bcb2e624310bf.histpack

Enable sendtrees and verify flat is converted to tree on demand
  $ cat >> $HGRCPATH <<EOF
  > [treemanifest]
  > sendtrees=True
  > EOF
  $ hg log -r 1 --stat
  changeset:   1:638af8a2d15f
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     flat only commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)
  $ hg repack

Transition to tree-only client
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF

Make a local tree-only draft commit
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*4 (re)
  $ ls .hg/store/packs/manifests > $TESTTMP/origpacks
  $ echo t >> subdir/x
  $ hg commit -qm "tree only commit"
  $ ls .hg/store/packs/manifests > $TESTTMP/aftercommit1packs
  $ hg debugdata -c 3
  7fdb5a91151d114ca83c30c5cb4a1029ef9700ef
  test
  0 0
  subdir/x
  
  tree only commit (no-eol)
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)
# No manifest revlog revision was added
  $ hg debugindex -m --config treemanifest.treeonly=False
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      51     -1       1 c0196aba344d 85b359fdb09e 000000000000
       2       102      51     -1       2 0427baa4e948 85b359fdb09e 000000000000

Tree-only amend
  $ echo >> subdir/x
  $ hg commit --amend
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/779a137458d0-0b309c3a-amend.hg (glob)
# amend commit was added
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*12 (re)
# No manifest revlog revision was added
  $ hg debugindex -m --config treemanifest.treeonly=False
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      51     -1       1 c0196aba344d 85b359fdb09e 000000000000
       2       102      51     -1       2 0427baa4e948 85b359fdb09e 000000000000

# Delete the original commits packs
  $ cd .hg/store/packs/manifests
  $ rm -rf `comm -3 $TESTTMP/origpacks $TESTTMP/aftercommit1packs`
  $ cd ../../../..
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*8 (re)

Test looking at the tree from inside the bundle
  $ hg log -r tip -vp -R $TESTTMP/client/.hg/strip-backup/779a137458d0-0b309c3a-amend.hg --pager=off
  changeset:   4:779a137458d0
  tag:         tip
  parent:      0:2278cc8c6ce6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  files:       subdir/x
  description:
  tree only commit
  
  
  diff -r 2278cc8c6ce6 -r 779a137458d0 subdir/x
  --- a/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  +++ b/subdir/x	Thu Jan 01 00:00:00 1970 +0000
  @@ -1,1 +1,2 @@
   x
  +t
  

Test unbundling the original commit
# Bring the original commit back from the bundle
  $ hg unbundle $TESTTMP/client/.hg/strip-backup/779a137458d0-0b309c3a-amend.hg
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 779a137458d0
# Verify the packs were brought back and the data is accessible
  $ ls_l .hg/store/packs/manifests | wc -l
  \s*12 (re)
  $ hg log -r tip --stat
  changeset:   4:779a137458d0
  tag:         tip
  parent:      0:2278cc8c6ce6
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
  d9920715ba88cbc7962c4dac9f20004aafd94ac8 (no-eol)

  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 2937cde31c19

  $ hg debugindex -m --config treemanifest.treeonly=False
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      51     -1       1 c0196aba344d 85b359fdb09e 000000000000
       2       102      51     -1       2 0427baa4e948 85b359fdb09e 000000000000
  $ hg log -r tip --stat --pager=off
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  changeset:   5:2937cde31c19
  tag:         tip
  parent:      0:2278cc8c6ce6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify subdir/x
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Test rebasing treeonly commits
  $ hg rebase -d 5 -b 2
  rebasing 2:4b702090309e "hybrid flat+tree commit"
  merging subdir/x
  warning: 1 conflicts while merging subdir/x! (edit, then use 'hg resolve --mark')
  unresolved conflicts (see hg resolve, then hg rebase --continue)
  [1]
  $ printf "x\nx\nh\n" > subdir/x
  $ hg resolve --mark subdir/x
  (no more unresolved files)
  continue: hg rebase --continue
  $ hg rebase --continue
  rebasing 2:4b702090309e "hybrid flat+tree commit"
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/4b702090309e-7a0f0c5f-rebase.hg (glob)

Test histedit treeonly commits
  $ hg up -q 38a88da6315b
  $ hg purge --config extensions.purge=
  $ echo y > y
  $ hg commit -Aqm 'add y'
  $ hg histedit --config extensions.histedit= --commands - <<EOF
  > pick eea609e344ca add y
  > pick 38a88da6315b hybrid flat+tree commit
  > EOF
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/38a88da6315b-320da9c1-histedit.hg (glob)
  $ hg log -l 2 -G -T '{desc}'
  @  hybrid flat+tree commit
  |
  o  add y
  |
  ~

Test {manifest} template
  $ hg log -r . -T '{manifest}'
  0ab0ab59dd6ab41bda558ad2fd4c665da69323ab (no-eol)

Test turning treeonly off and making sure we can still commit on top of treeonly
commits
  $ echo >> subdir/x
  $ hg debugindex -m --config treemanifest.treeonly=False | tail -1
       2       102      51     -1       2 0427baa4e948 85b359fdb09e 000000000000
  $ hg commit -m 'treeonly from hybrid repo' --config treemanifest.treeonly=False
  $ hg log -r . -T '{desc}\n' --stat
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg log -r . -T '{desc}\n' --stat --config treemanifest.treeonly=False
  treeonly from hybrid repo
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg debugindex -m --config treemanifest.treeonly=False | tail -1
       2       102      51     -1       2 0427baa4e948 85b359fdb09e 000000000000
  $ hg debugstrip -r .
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/client/.hg/strip-backup/87da9865954c-3cfa5389-backup.hg (glob)

Test peer-to-peer push/pull of tree only commits
  $ cd ..
  $ clearcache
  $ hgcloneshallow ssh://user@dummy/master client2 -q --config treemanifest.treeonly=True --config extensions.treemanifest=
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, found via 2937cde31c19
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client2
  $ ls_l .hg/store
  -rw-r--r--     277 00changelog.i
  drwxr-xr-x         data
  -rw-r--r--       0 requires
  -rw-r--r--       0 undo
  -rw-r--r--       2 undo.backupfiles
  -rw-r--r--       0 undo.phaseroots
  $ rm -rf $CACHEDIR
  $ cp ../client/.hg/hgrc .hg/hgrc

# Test pulling from a treeonly peer
# - We should see one tree recieve from the client, and then a second one when
#   prefetching the draft commit parent.
  $ hg pull -r tip ssh://user@dummy/client --debug 2>&1 | egrep "(payload|treegroup|running)"
  running python "*" 'user@dummy' 'hg -R client serve --stdio' (glob)
  bundle2-input-part: total payload size 827
  bundle2-input-part: total payload size 48
  bundle2-input-part: "b2x:treegroup2" (params: 3 mandatory) supported
  bundle2-input-part: total payload size 663
  running python "*" 'user@dummy' 'hg -R master serve --stdio' (glob)
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
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd, based on d9920715ba88cbc7962c4dac9f20004aafd94ac8
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
  new changesets 7ec3c5c54734:2f8e443c6ba8
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
  remote: +++ hg log -r 7ec3c5c54734448e59a0694af54c51578ee4d4de -T '{file_adds}'
  remote: ++ [[ y == \y ]]
  remote: ++ exit 1
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
  remote:     7ec3c5c54734  add y
  remote: 1 new changeset from the server will be downloaded
  adding changesets
  add changeset 4f84204095e0
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)

  $ cd ../master
- Delete the temporary commit we made earlier
  $ hg debugstrip -qr 2

- Verify the received tree was written down as a flat
  $ hg debugindex -m
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      51     -1       0 85b359fdb09e 000000000000 000000000000
       1        51      51     -1       1 d9920715ba88 85b359fdb09e 000000000000
       2       102      55      1       2 83b03df1c9d6 d9920715ba88 000000000000
  $ hg debugindex .hg/store/00manifesttree.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      50     -1       0 85b359fdb09e 000000000000 000000000000
       1        50      50     -1       1 d9920715ba88 85b359fdb09e 000000000000
       2       100      55      1       2 83b03df1c9d6 d9920715ba88 000000000000
- Verify the manifest data is accessible
  $ hg log -r tip --stat
  changeset:   2:4f84204095e0
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     add y
  
   y |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  

Test prefetch
  $ cd ../client
  $ clearcache
  $ hg prefetch -r 0
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ clearcache

Switch back to hybrid mode
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=False
  > EOF
  $ cp .hg/store/00manifest.i .hg/store/00manifest.i.bak
  $ cp .hg/store/00changelog.i .hg/store/00changelog.i.bak

- Test that accessing a public commit doesnt require the flat manifest
  $ clearcache
  $ hg log -r 'last(public())' --stat
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd
  2 trees fetched over * (glob)
  changeset:   4:2937cde31c19
  parent:      0:2278cc8c6ce6
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     modify subdir/x
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  2 files fetched over 1 fetches - (2 misses, 0.00% hit ratio) over * (glob)


  $ clearcache

- Force a draft commit to be public, to test if the backfill logic correctly
- filters it from discovery with the server.
  $ hg phase -pr 779a137458d0

- Auto-backfill on pull
  $ hg pull
  backfilling missing flat manifests
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 4f84204095e0

  $ cp .hg/store/00manifest.i.bak .hg/store/00manifest.i
  $ cp .hg/store/00changelog.i.bak .hg/store/00changelog.i
  $ hg phase -dfr 779a137458d0

- Manually backfill via command
  $ hg backfillmanifestrevlog
  adding changesets
  adding manifests
  adding file changes
  added 0 changesets with 0 changes to 0 files

  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 1 changesets with 0 changes to 0 files (+1 heads)
  new changesets 4f84204095e0
  $ cd ..

Test converting server to treeonly
  $ cd master
  $ cp .hg/hgrc .hg/hgrc.bak
  $ cat >> .hg/hgrc <<EOF
  > [treemanifest]
  > treeonly=True
  > EOF
- Move the flat manifest away so we guarantee its not read
  $ mv .hg/store/00manifest.i .hg/store/00manifest.i_old
  $ hg log -G -T "{desc}\n" --stat
  o  add y
  |   y |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
  |
  @  modify subdir/x
  |   subdir/x |  1 +
  |   1 files changed, 1 insertions(+), 0 deletions(-)
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
  
Test pulling to a flat client from a treeonly server
  $ cd ../client
  $ hg pull
  pulling from ssh://user@dummy/master
  abort: non-treemanifest clients cannot pull from treemanifest-only servers
  [255]

Test pushing flat manifests to a treeonly server
- Update to a commit with a flat manifest
  $ hg up -q 2937cde31
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, found via 2937cde31c19
  2 trees fetched over * (glob)
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ echo unpushable >> subdir/x
  $ hg commit -m "unpushable commit"
  $ hg push -r tip --to master --config treemanifest.sendtrees=False
  pushing to ssh://user@dummy/master
  searching for changes
  remote: "unable to find the following nodes locally or on the server: ('', 89bffa38cf192d8f8a234bfb14dd22d0c65064f0)"
  abort: push failed on remote
  [255]

Test pulling to a treeonly client from a treeonly server
  $ cd ../client2
  $ hg pull
  pulling from ssh://user@dummy/master
  searching for changes
  adding changesets
  adding manifests
  adding file changes
  added 2 changesets with 0 changes to 0 files (+1 heads)
  new changesets 4f84204095e0:5b1ec8639460
  $ hg log -r tip -T '{desc}\n' --stat
  fetching tree '' 83b03df1c9d62b8a2dedf46629e3262423af655c, based on d9920715ba88cbc7962c4dac9f20004aafd94ac8, found via 4f84204095e0
  1 trees fetched over * (glob)
  fetching tree '' bd5ff58fa887770ff0ea29dde0b91f5804cdeff0, based on 83b03df1c9d62b8a2dedf46629e3262423af655c, found via 4f84204095e0
  2 trees fetched over * (glob)
  modify subdir/x again
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)

Test pushing from a treeonly client to a treeonly server
  $ hg config treemanifest
  treemanifest.flatcompat=False
  treemanifest.sendtrees=True
  treemanifest.demanddownload=True
  treemanifest.treeonly=True
  $ echo 'pushable' >> subdir/x
  $ hg commit -Aqm 'pushable treeonly commit'

Test pushing from a treeonly client to a treeonly server *without* pushrebase

  $ hg log -Gf -l 4 -T '{shortest(node)} {manifest|short}\n'
  @  11c4 14e2f802690d
  |
  o  2f8e e23b32620909
  |
  o  5b48 0ab0ab59dd6a
  |
  o  7ec3 ad4ae432d47f
  |
  ~
  $ hg push -r . --config extensions.pushrebase=! -f
  pushing to ssh://user@dummy/master
  searching for changes
  remote: adding changesets
  remote: adding manifests
  remote: adding file changes
  remote: added 4 changesets with 3 changes to 2 files (+1 heads)
  $ hg --cwd ../master debugindex .hg/store/00manifesttree.i | tail -4
       4       216      55      1       4 ad4ae432d47f d9920715ba88 000000000000
       5       271      61      4       5 0ab0ab59dd6a ad4ae432d47f 000000000000
       6       332      80     -1       6 e23b32620909 0ab0ab59dd6a 000000000000
       7       412      60      6       7 14e2f802690d e23b32620909 000000000000
  $ hg --cwd ../master debugindex .hg/store/meta/subdir/00manifest.i
     rev    offset  length  delta linkrev nodeid       p1           p2
       0         0      44     -1       0 bc0c2c938b92 000000000000 000000000000
       1        44      44     -1       1 a18d21674e76 bc0c2c938b92 000000000000
       2        88      44     -1       3 33b5c6e3c136 a18d21674e76 000000000000
       3       132      44     -1       5 f4c373af9a41 a18d21674e76 000000000000
       4       176      44     -1       7 d20854ad7783 f4c373af9a41 000000000000
  $ hg -R ../master log -r tip --stat
  changeset:   7:11c4fc95a874
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
  remote: added 4 changesets with 3 changes to 2 files (+1 heads)
  $ hg -R ../master log -r tip --stat
  changeset:   7:11c4fc95a874
  tag:         tip
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     pushable treeonly commit
  
   subdir/x |  1 +
   1 files changed, 1 insertions(+), 0 deletions(-)
  
  $ hg -R ../master debugstrip -r tip~3
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/7ec3c5c54734-d6767557-backup.hg
  $ hg phase -dfr tip~3

Test pushing from a treeonly client to a treeonly server *with* pushrebase

  $ hg push --to master
  pushing to ssh://user@dummy/master
  searching for changes
  remote: pushing 4 changesets:
  remote:     7ec3c5c54734  add y
  remote:     5b483416c8aa  hybrid flat+tree commit
  remote:     2f8e443c6ba8  modify y
  remote:     11c4fc95a874  pushable treeonly commit
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
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  saved backup bundle to $TESTTMP/master/.hg/strip-backup/7ec3c5c54734-3e715521-backup.hg

Reset the phase of the local commits to draft
  $ hg phase -fd 2::

Test histedit with changing commits in the middle
  $ cat >> $TESTTMP/commands <<EOF
  > pick 2f8e443c6ba8 4
  > x echo >> y && hg amend
  > pick 11c4fc95a874 7
  > EOF
  $ hg histedit '.^' --commands $TESTTMP/commands --config extensions.histedit= --config extensions.fbhistedit=
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  warning: orphaned descendants detected, not stripping 2f8e443c6ba8
  saved backup bundle to $TESTTMP/client2/.hg/strip-backup/11c4fc95a874-b05d0d47-histedit.hg

Reset the server back to hybrid mode
  $ cd ../master
  $ mv .hg/hgrc.bak .hg/hgrc
  $ cd ..

Test creating a treeonly repo from scratch
  $ hg init treeonlyrepo
  $ cd treeonlyrepo
  $ cat >> .hg/hgrc <<EOF
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > 
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
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
  > [extensions]
  > treemanifest=
  > fastmanifest=
  > 
  > [treemanifest]
  > sendtrees=True
  > treeonly=True
  > pullprefetchrevs=tip
  > 
  > [fastmanifest]
  > usetree=True
  > usecache=False
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
  new changesets 2278cc8c6ce6:4f84204095e0
  prefetching tree for 4f84204095e0
  2 trees fetched over * (glob)
  $ cd ..

Test ondemand downloading trees with a limited depth
  $ hgcloneshallow ssh://user@dummy/master client4 -q --config treemanifest.treeonly=True --config extensions.treemanifest=
  1 files fetched over 1 fetches - (1 misses, 0.00% hit ratio) over * (glob)
  $ cd client4
  $ cp ../client/.hg/hgrc .hg/hgrc

  $ clearcache
  $ hg status --change 'tip^'
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 2937cde31c19
  2 trees fetched over * (glob)
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 2937cde31c19
  2 trees fetched over * (glob)
  M subdir/x

  $ clearcache
  $ hg status --change 'tip^' --config treemanifest.fetchdepth=1
  fetching tree '' 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 2937cde31c19
  1 trees fetched over * (glob)
  fetching tree '' d9920715ba88cbc7962c4dac9f20004aafd94ac8, based on 85b359fdb09e9b8d7ac4a74551612b277345e8fd, found via 2937cde31c19
  1 trees fetched over * (glob)
  fetching tree 'subdir' bc0c2c938b929f98b1c31a8c5994396ebb096bf0
  1 trees fetched over * (glob)
  fetching tree 'subdir' a18d21674e76d6aab2edb46810b20fbdbd10fb4b
  1 trees fetched over * (glob)
  M subdir/x
