#chg-compatible
  $ configure modernclient
  $ enable convert

  $ glog()
  > {
  >     hg log -G --template '{node|short} "{desc|firstline}"\
  >  files: {files}\n' "$@"
  > }
  $ newclientrepo repo1
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
  $ glog
  @  e55c719b85b6 "addc" files: c
  │
  o  6d4c2037ddc2 "addb" files: a b
  │
  o  07f494440405 "adda" files: a
  
  $ hg push -q -r tip --to book --create
  $ cd ..

  $ newclientrepo repo2
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
  @  a39b65753b0a "adde" files: e
  │
  o  e4ea00df9189 "changed" files: d
  │
  o  527cdedf31fb "addaandd" files: a d
  

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
  
  $ newclientrepo target1 test:repo1_server book
  $ cd ..
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
  o  16bc847b02aa "adde" files: e
  │
  o    e30e4fee3418 "changed" files: d
  ├─╮
  │ o  e673348c3a3c "addaandd" files: a d
  │ │
  @ │  e55c719b85b6 "addc" files: c
  ├─╯
  o  6d4c2037ddc2 "addb" files: a b
  │
  o  07f494440405 "adda" files: a
  



Test splicemap and conversion order

  $ newclientrepo ordered
  $ echo a > a
  $ hg ci -Am adda
  adding a
  $ echo a >> a
  $ hg ci -Am changea
  $ echo a >> a
  $ hg ci -Am changeaagain
  $ hg up 'desc(adda)'
  1 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ echo b > b
  $ hg ci -Am addb
  adding b
  $ glog
  @  102a90ea7b4a "addb" files: b
  │
  │ o  67b3901ca07e "changeaagain" files: a
  │ │
  │ o  540395c44225 "changea" files: a
  ├─╯
  o  07f494440405 "adda" files: a
  

We want 2 to depend on 1 and 3. Since 3 is always converted after 2,
the bug should be exhibited with all conversion orders.

  $ cat > ../splicemap <<EOF
  > `(hg id -r 'desc(changeaagain)' -i --debug)` `(hg id -r 'first(desc(changea))' -i --debug)`, `(hg id -r 'desc(addb)' -i --debug)`
  > EOF
  $ cd ..
  $ cat splicemap
  67b3901ca07e1edc2eeb0971ed1e4647833ec555 540395c442253af3b991be882b539e7e198b5808, 102a90ea7b4a3361e4082ed620918c261189a36a

Test regular conversion

  $ newclientrepo ordered-hg1
  $ cd ..
  $ hg convert --splicemap splicemap ordered ordered-hg1
  scanning source...
  sorting...
  converting...
  3 adda
  2 changea
  1 addb
  0 changeaagain
  spliced in 540395c442253af3b991be882b539e7e198b5808 and 102a90ea7b4a3361e4082ed620918c261189a36a as parents of 67b3901ca07e1edc2eeb0971ed1e4647833ec555
  $ glog -R ordered-hg1
  o    e87a37405c69 "changeaagain" files: a
  ├─╮
  │ o  102a90ea7b4a "addb" files: b
  │ │
  o │  540395c44225 "changea" files: a
  ├─╯
  o  07f494440405 "adda" files: a
  

Test conversion with parent revisions already in dest, using source
and destination identifiers. Test unknown splicemap target.

  $ newclientrepo ordered-hg2
  $ cd ..
  $ hg convert -r 540395c44225 ordered ordered-hg2
  scanning source...
  sorting...
  converting...
  1 adda
  0 changea
  $ hg convert -r 102a90ea7b4a ordered ordered-hg2
  scanning source...
  sorting...
  converting...
  0 addb
  $ cat > splicemap <<EOF
  > `(hg -R ordered id -r 'desc(changeaagain)' -i --debug)` \
  > `(hg -R ordered-hg2 id -r 'desc(adda)' -i --debug)`,\
  > `(hg -R ordered-hg2 id -r 'desc(addb)' -i --debug)`
  > deadbeef102a90ea7b4a3361e4082ed620918c26 deadbeef102a90ea7b4a3361e4082ed620918c27
  > EOF
  $ hg convert --splicemap splicemap ordered ordered-hg2
  scanning source...
  splice map revision deadbeef102a90ea7b4a3361e4082ed620918c26 is not being converted, ignoring
  sorting...
  converting...
  0 changeaagain
  spliced in 07f4944404050f47db2e5c5071e0e84e7a27bba9 and 102a90ea7b4a3361e4082ed620918c261189a36a as parents of 67b3901ca07e1edc2eeb0971ed1e4647833ec555
  $ glog -R ordered-hg2
  o    0a1baec1d545 "changeaagain" files: a
  ├─╮
  │ o  102a90ea7b4a "addb" files: b
  ├─╯
  │ o  540395c44225 "changea" files: a
  ├─╯
  o  07f494440405 "adda" files: a
  

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
  abort: revision 07f4944404050f47db2e5c5071e0e84e7a27bba9 not found in destination repository (lookups with clonebranches=true are not implemented)
  [255]

Test invalid dependency

  $ cat > splicemap <<EOF
  > `(hg -R ordered id -r 'desc(changeaagain)' -i --debug)` \
  > deadbeef102a90ea7b4a3361e4082ed620918c26,\
  > `(hg -R ordered-hg2 id -r 'desc(changeaagain)' -i --debug)`
  > EOF
  $ newclientrepo ordered-hg4
  $ cd ..
  $ hg convert --splicemap splicemap ordered ordered-hg4
  scanning source...
  abort: unknown splice map parent: deadbeef102a90ea7b4a3361e4082ed620918c26
  [255]
