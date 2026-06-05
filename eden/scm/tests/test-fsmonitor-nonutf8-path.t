#require fsmonitor linux

  $ configure modernclient
  $ newclientrepo repo

sl status on a non-utf8 filename
  $ touch foo
  $ python -c 'open(b"\xc3\x28", "wb+").write(b"asdf")'
  $ sl status
  skipping invalid filename: '\xC3('
  ? foo
