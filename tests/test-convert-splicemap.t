
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ glog()
  > {
  >     hg log -G --template '{rev}:{node|short} "{desc|firstline}"\
  >  files: {files}\n' "$@"
  > }
  $ hg init repo1
  $ cd repo1
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo b > b
  $ echo a >> a
  $ hg ci -Am addb
  adding b
  $ PARENTID1=`hg id --debug -i`
  $ echo c > c
  $ hg ci -Am addc
  adding c
  $ PARENTID2=`hg id --debug -i`
  $ cd ..
  $ glog -R repo1
  @  2:e55c719b85b6 "addc" files: c
  |
  o  1:6d4c2037ddc2 "addb" files: a b
  |
  o  0:07f494440405 "adda" files: a
  

  $ hg init repo2
  $ cd repo2
  $ echo b > a
  $ echo d > d
  $ hg ci -Am addaandd
  adding a
  adding d
  $ INVALIDID1=afd12345af
  $ INVALIDID2=28173x36ddd1e67bf7098d541130558ef5534a86
  $ CHILDID1=`hg id --debug -i`
  $ echo d >> d
  $ hg ci -Am changed
  $ CHILDID2=`hg id --debug -i`
  $ echo e > e
  $ hg ci -Am adde
  adding e
  $ cd ..
  $ glog -R repo2
  @  2:a39b65753b0a "adde" files: e
  |
  o  1:e4ea00df9189 "changed" files: d
  |
  o  0:527cdedf31fb "addaandd" files: a d
  

test invalid splicemap1

  $ cat > splicemap <<EOF
  > $CHILDID2
  > EOF
  $ hg convert --splicemap splicemap repo2 repo1
  abort: syntax error in splicemap(1): child parent1[,parent2] expected
  [255]

test invalid splicemap2

  $ cat > splicemap <<EOF
  > $CHILDID2 $PARENTID1, $PARENTID2, $PARENTID2
  > EOF
  $ hg convert --splicemap splicemap repo2 repo1
  abort: syntax error in splicemap(1): child parent1[,parent2] expected
  [255]

test invalid splicemap3

  $ cat > splicemap <<EOF
  > $INVALIDID1 $INVALIDID2
  > EOF
  $ hg convert --splicemap splicemap repo2 repo1
  abort: splicemap entry afd12345af is not a valid revision identifier
  [255]

splice repo2 on repo1

  $ cat > splicemap <<EOF
  > $CHILDID1 $PARENTID1
  > $CHILDID2 $PARENTID2,$CHILDID1
  > 
  > EOF
  $ cat splicemap
  527cdedf31fbd5ea708aa14eeecf53d4676f38db 6d4c2037ddc2cb2627ac3a244ecce35283268f8e
  e4ea00df91897da3079a10fab658c1eddba6617b e55c719b85b60e5102fac26110ba626e7cb6b7dc,527cdedf31fbd5ea708aa14eeecf53d4676f38db
  
  $ hg clone repo1 target1
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg convert --splicemap splicemap repo2 target1
  scanning source...
  sorting...
  converting...
  2 addaandd
  spliced in 6d4c2037ddc2cb2627ac3a244ecce35283268f8e as parents of 527cdedf31fbd5ea708aa14eeecf53d4676f38db
  1 changed
  spliced in e55c719b85b60e5102fac26110ba626e7cb6b7dc and 527cdedf31fbd5ea708aa14eeecf53d4676f38db as parents of e4ea00df91897da3079a10fab658c1eddba6617b
  0 adde
  $ glog -R target1
  o  5:16bc847b02aa "adde" files: e
  |
  o    4:e30e4fee3418 "changed" files: d
  |\
  | o  3:e673348c3a3c "addaandd" files: a d
  | |
  @ |  2:e55c719b85b6 "addc" files: c
  |/
  o  1:6d4c2037ddc2 "addb" files: a b
  |
  o  0:07f494440405 "adda" files: a
  



Test splicemap and conversion order

  $ hg init ordered
  $ cd ordered
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ hg branch branch
  marked working directory as branch branch
  (branches are permanent and global, did you want a bookmark?)
  $ echo a >> a
  $ hg ci -Am changea
  $ echo a >> a
  $ hg ci -Am changeaagain
  $ hg up 0
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am addb
  adding b

We want 2 to depend on 1 and 3. Since 3 is always converted after 2,
the bug should be exhibited with all conversion orders.

  $ cat > ../splicemap <<EOF
  > `(hg id -r 2 -i --debug)` `(hg id -r 1 -i --debug)`, `(hg id -r 3 -i --debug)`
  > EOF
  $ cd ..
  $ cat splicemap
  7c364e7fa7d70ae525610c016317ed717b519d97 717d54d67e6c31fd75ffef2ff3042bdd98418437, 102a90ea7b4a3361e4082ed620918c261189a36a

Test regular conversion

  $ hg convert --splicemap splicemap ordered ordered-hg1
  initializing destination ordered-hg1 repository
  scanning source...
  sorting...
  converting...
  3 adda
  2 changea
  1 addb
  0 changeaagain
  spliced in 717d54d67e6c31fd75ffef2ff3042bdd98418437 and 102a90ea7b4a3361e4082ed620918c261189a36a as parents of 7c364e7fa7d70ae525610c016317ed717b519d97
  $ glog -R ordered-hg1
  o    3:4cb04b9afbf2 "changeaagain" files: a
  |\
  | o  2:102a90ea7b4a "addb" files: b
  | |
  o |  1:717d54d67e6c "changea" files: a
  |/
  o  0:07f494440405 "adda" files: a
  

Test conversion with parent revisions already in dest, using source
and destination identifiers. Test unknown splicemap target.

  $ hg convert -r1 ordered ordered-hg2
  initializing destination ordered-hg2 repository
  scanning source...
  sorting...
  converting...
  1 adda
  0 changea
  $ hg convert -r3 ordered ordered-hg2
  scanning source...
  sorting...
  converting...
  0 addb
  $ cat > splicemap <<EOF
  > `(hg -R ordered id -r 2 -i --debug)` \
  > `(hg -R ordered-hg2 id -r 1 -i --debug)`,\
  > `(hg -R ordered-hg2 id -r 2 -i --debug)`
  > deadbeef102a90ea7b4a3361e4082ed620918c26 deadbeef102a90ea7b4a3361e4082ed620918c27
  > EOF
  $ hg convert --splicemap splicemap ordered ordered-hg2
  scanning source...
  splice map revision deadbeef102a90ea7b4a3361e4082ed620918c26 is not being converted, ignoring
  sorting...
  converting...
  0 changeaagain
  spliced in 717d54d67e6c31fd75ffef2ff3042bdd98418437 and 102a90ea7b4a3361e4082ed620918c261189a36a as parents of 7c364e7fa7d70ae525610c016317ed717b519d97
  $ glog -R ordered-hg2
  o    3:4cb04b9afbf2 "changeaagain" files: a
  |\
  | o  2:102a90ea7b4a "addb" files: b
  | |
  o |  1:717d54d67e6c "changea" files: a
  |/
  o  0:07f494440405 "adda" files: a
  

Test empty conversion

  $ hg convert --splicemap splicemap ordered ordered-hg2
  scanning source...
  splice map revision deadbeef102a90ea7b4a3361e4082ed620918c26 is not being converted, ignoring
  sorting...
  converting...

Test clonebranches

  $ hg --config convert.hg.clonebranches=true convert \
  >   --splicemap splicemap ordered ordered-hg3
  initializing destination ordered-hg3 repository
  scanning source...
  abort: revision 717d54d67e6c31fd75ffef2ff3042bdd98418437 not found in destination repository (lookups with clonebranches=true are not implemented)
  [255]

Test invalid dependency

  $ cat > splicemap <<EOF
  > `(hg -R ordered id -r 2 -i --debug)` \
  > deadbeef102a90ea7b4a3361e4082ed620918c26,\
  > `(hg -R ordered-hg2 id -r 2 -i --debug)`
  > EOF
  $ hg convert --splicemap splicemap ordered ordered-hg4
  initializing destination ordered-hg4 repository
  scanning source...
  abort: unknown splice map parent: deadbeef102a90ea7b4a3361e4082ed620918c26
  [255]
