#require fsmonitor linux

#testcases pythonstatus ruststatus

#if pythonstatus
  $ setconfig workingcopy.rust-status=false status.use-rust=false
#endif

#if ruststatus
  $ setconfig status.use-rust=true
#endif

  $ configure modernclient
  $ newclientrepo repo

hg status on a non-utf8 filename
  $ touch foo
  $ python3 -c 'open(b"\xc3\x28", "wb+").write(b"asdf")'
  $ hg status --traceback
  skipping * filename: '*' (glob)
  ? foo
