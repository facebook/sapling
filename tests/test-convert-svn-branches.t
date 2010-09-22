
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ cat > $HGRCPATH <<EOF
  > [extensions]
  > convert = 
  > graphlog =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/branches.svndump"

Convert trunk and branches

  $ cat > branchmap <<EOF
  > old3 newbranch
  > EOF
  $ hg convert --branchmap=branchmap --datesort -r 10 svn-repo A-hg
  initializing destination A-hg repository
  scanning source...
  sorting...
  converting...
  10 init projA
  9 hello
  8 branch trunk, remove c and dir
  7 change a
  6 change b
  5 move and update c
  4 move and update c
  3 change b again
  2 move to old2
  1 move back to old
  0 last change to a

Convert again

  $ hg convert --branchmap=branchmap --datesort svn-repo A-hg
  scanning source...
  sorting...
  converting...
  0 branch trunk@1 into old3

  $ cd A-hg
  $ hg glog --template 'branch={branches} {rev} {desc|firstline} files: {files}\n'
  o  branch=newbranch 11 branch trunk@1 into old3 files:
  |
  | o  branch= 10 last change to a files: a
  | |
  | | o  branch=old 9 move back to old files:
  | | |
  | | o  branch=old2 8 move to old2 files:
  | | |
  | | o  branch=old 7 change b again files: b
  | | |
  | o |  branch= 6 move and update c files: b
  | | |
  | | o  branch=old 5 move and update c files: c
  | | |
  | | o  branch=old 4 change b files: b
  | | |
  | o |  branch= 3 change a files: a
  | | |
  | | o  branch=old 2 branch trunk, remove c and dir files: c
  | |/
  | o  branch= 1 hello files: a b c dir/e
  |/
  o  branch= 0 init projA files:
  

  $ hg branches
  newbranch                     11:08fca3ff8634
  default                       10:098988aa63ba
  old                            9:b308f345079b
  old2                           8:49f2336c7b8b (inactive)
  $ hg tags -q
  tip
  $ cd ..

Test hg failing to call itself

  $ HG=foobar hg convert svn-repo B-hg
  * (glob)
  initializing destination B-hg repository
  abort: Mercurial failed to run itself, check hg executable is in PATH
  [255]

