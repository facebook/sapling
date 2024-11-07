
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

create a test directory

  $ mkdir $TESTTMP/wcrepo/test_dir

eden remove this directory should also see error about Determination state

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo/test_dir
  Error: Rust remove(Determination) is not implemented!
  [1]

#if linuxormacos
remove wcrepo with eden rust cli should succeed

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo

check to make sure the mount point is cleanly removed
  $ ls $TESTTMP/wcrepo
  ls: $TESTTMP/wcrepo: $ENOENT$
  [1]

#endif

#if windows
remove wcrepo with eden rust cli should see error about CleanUp for Windows not implemented

  $ EDENFSCTL_ONLY_RUST=true eden remove -y $TESTTMP/wcrepo
  Error: Failed to clean mount point $TESTTMP/wcrepo
  
  Caused by:
      Windows clean_mount_point not implemented!!
  [1]

#endif
