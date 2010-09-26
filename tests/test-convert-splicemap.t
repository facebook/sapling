
  $ echo "[extensions]" >> $HGRCPATH
  $ echo "convert=" >> $HGRCPATH
  $ echo 'graphlog =' >> $HGRCPATH
  $ glog()
  > {
  >     hg glog --template '{rev} "{desc|firstline}" files: {files}\n' "$@"
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
  $ hg init repo2
  $ cd repo2
  $ echo b > a
  $ echo d > d
  $ hg ci -Am addaandd
  adding a
  adding d
  $ CHILDID1=`hg id --debug -i`
  $ echo d >> d
  $ hg ci -Am changed
  $ CHILDID2=`hg id --debug -i`
  $ echo e > e
  $ hg ci -Am adde
  adding e
  $ cd ..

test invalid splicemap

  $ cat > splicemap <<EOF
  > $CHILDID2
  > EOF
  $ hg convert --splicemap splicemap repo2 repo1
  abort: syntax error in splicemap(1): key/value pair expected
  [255]

splice repo2 on repo1

  $ cat > splicemap <<EOF
  > $CHILDID1 $PARENTID1
  > $CHILDID2 $PARENTID2,$CHILDID1
  > EOF
  $ hg clone repo1 target1
  updating to branch default
  3 files updated, 0 files merged, 0 files removed, 0 files unresolved
  $ hg convert --splicemap splicemap repo2 target1
  scanning source...
  sorting...
  converting...
  2 addaandd
  spliced in ['6d4c2037ddc2cb2627ac3a244ecce35283268f8e'] as parents of 527cdedf31fbd5ea708aa14eeecf53d4676f38db
  1 changed
  spliced in ['e55c719b85b60e5102fac26110ba626e7cb6b7dc', '527cdedf31fbd5ea708aa14eeecf53d4676f38db'] as parents of e4ea00df91897da3079a10fab658c1eddba6617b
  0 adde
  $ glog -R target1
  o  5 "adde" files: e
  |
  o    4 "changed" files: d
  |\
  | o  3 "addaandd" files: a d
  | |
  @ |  2 "addc" files: c
  |/
  o  1 "addb" files: a b
  |
  o  0 "adda" files: a
  
