#debugruntest-compatible
#require fsmonitor

  $ configure modernclient
  $ enable sparse

  $ newclientrepo
  $ hg sparse include include

  $ mkdir include
  $ echo foo > include/include
  $ echo foo > exclude
  $ hg st
  ? include/include
  $ hg commit -Aqm foo

Make sure we aren't tracking "exclude" yet.
  $ hg debugtreestate list
  include/include: * (glob)

Now we should.
  $ setconfig fsmonitor.track-ignore-files=true
  $ hg st
  $ hg debugtreestate list
  exclude: * (glob)
  include/include: * (glob)
