#require fsmonitor

  $ configure modernclient
  $ setconfig status.use-rust=False workingcopy.ruststatus=False
  $ newclientrepo repo

hg status on a non-utf8 filename
  $ touch foo
  $ python2 -c 'open(b"\xc3\x28", "wb+").write("asdf")'
  $ hg status --traceback
  skipping invalid utf-8 filename: '*' (glob)
  ? foo
