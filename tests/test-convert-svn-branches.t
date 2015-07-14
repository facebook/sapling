#require svn svn-bindings

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > convert =
  > EOF

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/branches.svndump"

Convert trunk and branches

  $ cat > branchmap <<EOF
  > old3 newbranch
  > 
  > 
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

Test template keywords

  $ hg -R A-hg log --template '{rev} {svnuuid}{svnpath}@{svnrev}\n'
  10 644ede6c-2b81-4367-9dc8-d786514f2cde/trunk@10
  9 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old@9
  8 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old2@8
  7 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old@7
  6 644ede6c-2b81-4367-9dc8-d786514f2cde/trunk@6
  5 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old@6
  4 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old@5
  3 644ede6c-2b81-4367-9dc8-d786514f2cde/trunk@4
  2 644ede6c-2b81-4367-9dc8-d786514f2cde/branches/old@3
  1 644ede6c-2b81-4367-9dc8-d786514f2cde/trunk@2
  0 644ede6c-2b81-4367-9dc8-d786514f2cde/trunk@1

Convert again

  $ hg convert --branchmap=branchmap --datesort svn-repo A-hg
  scanning source...
  sorting...
  converting...
  0 branch trunk@1 into old3

  $ cd A-hg
  $ hg log -G --template 'branch={branches} {rev} {desc|firstline} files: {files}\n'
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
  newbranch                     11:a6d7cc050ad1
  default                       10:6e2b33404495
  old                            9:93c4b0f99529
  old2                           8:b52884d7bead (inactive)
  $ hg tags -q
  tip
  $ cd ..

Test hg failing to call itself

  $ HG=foobar hg convert svn-repo B-hg 2>&1 | grep abort
  abort: Mercurial failed to run itself, check hg executable is in PATH

Convert 'trunk' to branch other than 'default'

  $ cat > branchmap <<EOF
  > default hgtrunk
  > 
  > 
  > EOF
  $ hg convert --branchmap=branchmap --datesort -r 10 svn-repo C-hg
  initializing destination C-hg repository
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

  $ cd C-hg
  $ hg branches --template '{branch}\n'
  hgtrunk
  old
  old2
  $ cd ..
