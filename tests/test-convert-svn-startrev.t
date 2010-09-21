
  $ "$TESTDIR/hghave" svn svn-bindings || exit 80

  $ cat > $HGRCPATH <<EOF
  > [extensions]
  > convert = 
  > graphlog =
  > EOF
  $ convert()
  > {
  >     startrev=$1
  >     repopath=A-r$startrev-hg
  >     hg convert --config convert.svn.startrev=$startrev \
  >         --config convert.svn.trunk=branches/branch1 \
  >         --config convert.svn.branches="  " \
  >         --config convert.svn.tags= \
  >         --datesort svn-repo $repopath
  >     hg -R $repopath glog \
  >         --template '{rev} {desc|firstline} files: {files}\n'
  >     echo
  > }

  $ svnadmin create svn-repo
  $ svnadmin load -q svn-repo < "$TESTDIR/svn/startrev.svndump"

Convert before branching point

  $ convert 3
  initializing destination A-r3-hg repository
  scanning source...
  sorting...
  converting...
  3 removeb
  2 changeaa
  1 branch, changeaaa
  0 addc,changeaaaa
  o  3 addc,changeaaaa files: a c
  |
  o  2 branch, changeaaa files: a
  |
  o  1 changeaa files: a
  |
  o  0 removeb files: a
  
  

Convert before branching point

  $ convert 4
  initializing destination A-r4-hg repository
  scanning source...
  sorting...
  converting...
  2 changeaa
  1 branch, changeaaa
  0 addc,changeaaaa
  o  2 addc,changeaaaa files: a c
  |
  o  1 branch, changeaaa files: a
  |
  o  0 changeaa files: a
  
  

Convert at branching point

  $ convert 5
  initializing destination A-r5-hg repository
  scanning source...
  sorting...
  converting...
  1 branch, changeaaa
  0 addc,changeaaaa
  o  1 addc,changeaaaa files: a c
  |
  o  0 branch, changeaaa files: a
  
  

Convert last revision only

  $ convert 6
  initializing destination A-r6-hg repository
  scanning source...
  sorting...
  converting...
  0 addc,changeaaaa
  o  0 addc,changeaaaa files: a c
  
  
