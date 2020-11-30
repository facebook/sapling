#require fsmonitor

  $ setconfig fsmonitor.warn-fresh-instance=true
  $ newrepo

A warning is printed for the first use

  $ hg status --debug
  warning: watchman has recently started (pid *) - operation will be slower than usual (glob)
  poststatusfixup decides to wait for wlock since watchman reported fresh instance

  $ hg status --debug

hg status on a non-utf8 filename
  $ touch foo
  $ python2 -c 'open(b"\xc3\x28", "wb+").write("asdf")'
  $ hg status --traceback
  skipping invalid utf-8 filename: '*' (glob)
  ? foo
