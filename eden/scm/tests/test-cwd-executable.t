
  $ configure modernclient

  $ newclientrepo
#if windows
  $ cat > watchman.bat <<EOF
  > type nul > oops
  > EOF
#else
  $ cat > watchman <<EOF
  > touch oops
  > EOF
  $ chmod +x watchman
#endif
  $ sl commit -Aqm foo
  $ touch bar
  $ sl commit -Aqm bar
This is the code under test - don't run the "watchman" in CWD.
  $ sl up -q .^
  $ sl status
