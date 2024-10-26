
#require eden

setup backing repo

  $ newclientrepo backingrepo
  $ eden clone --allow-empty-repo $TESTTMP/backingrepo $TESTTMP/wcrepo
  Cloning new repository at $TESTTMP/wcrepo...
  Success.  Checked out commit 00000000

touch a test file
  
  $ touch $TESTTMP/wcrepo/file.txt

eden remove this file should see error about RegFile state

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo/file.txt
  Error: Rust remove(RegFile) is not implemented!
  [1]

#if linuxormacos
create a test directory

  $ mkdir $TESTTMP/wcrepo/test_dir

eden remove this directory should also see error about Determination state

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo/test_dir
  Error: Rust remove(Determination) is not implemented!
  [1]

remove wcrepo with eden rust cli should see error about ActiveEdenMount state

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo
  Error: Rust remove(ActiveEdenMount) is not implemented!
  [1]
#endif
