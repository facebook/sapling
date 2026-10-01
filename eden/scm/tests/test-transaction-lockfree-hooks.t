#require no-eden

  $ configure dummyssh mutation-norecord
  $ enable commitcloud

  $ setconfig commitcloud.hostname=testhost
  $ setconfig remotefilelog.reponame=server
  $ setconfig devel.collapse-traceback=true

  $ newserver server
  $ touch base
  $ sl commit -Aqm base
  $ sl bookmark master
  $ sl debugmakepublic .
  $ cd ..

  $ sl clone ssh://user@dummy/server client -q
  $ cd client
  $ setconfig commitcloud.servicetype=local commitcloud.servicelocation=$TESTTMP
  $ sl cloud join -q

An external transaction hook is incompatible with lock-free cloud sync.

  $ touch draft
  $ sl commit -Aqm draft
  $ sl --config hooks.txnclose.repro=true cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  ...
  Traceback (most recent call last):
  ...
  sapling.error.ProgrammingError: unsupported in lockfree transaction
  [1]

The upload succeeded even though the command failed while preparing the hook.

  $ sl cloud sync
  commitcloud: synchronizing 'server' with 'user/test/default'
  commitcloud: nothing to upload
  commitcloud: commits synchronized
  finished in * (glob)
