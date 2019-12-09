#chg-compatible

(This test needs to re-run the hg process. Therefore hard to use single-process Python test)

Test indexedlogdatapack

  $ . "$TESTDIR/library.sh"

  $ newrepo master
  $ setconfig remotefilelog.server=true remotefilelog.serverexpiration=-1

  $ cd $TESTTMP
  $ setconfig remotefilelog.debug=false remotefilelog.indexedlogdatastore=true remotefilelog.fetchpacks=true

  $ hgcloneshallow ssh://user@dummy/master shallow -q
  $ cd shallow

 (Accessing repo.fileslog creates an empty store)
  $ hg debugshell -c 'repo.fileslog'

  $ hg doctor
  attempt to check and fix indexedlogdatastore ...
    Attempt to repair log "0"
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Latest = 0

  $ echo x > $TESTTMP/hgcache/master/indexedlogdatastore/latest
  $ hg doctor
  attempt to check and fix indexedlogdatastore ...
    Attempt to repair log "0"
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Reset latest to 0

  $ rm $TESTTMP/hgcache/master/indexedlogdatastore/latest
  $ hg doctor
  attempt to check and fix indexedlogdatastore ...
    Attempt to repair log "0"
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Reset latest to 0

  $ echo x > $TESTTMP/hgcache/master/indexedlogdatastore/0/log
  $ hg doctor
  attempt to check and fix indexedlogdatastore ...
    Attempt to repair log "0"
    Fixed header in log
    Verified 0 entries, 12 bytes in log
    Index "node" passed integrity check
    Latest = 0

  $ echo y > $TESTTMP/hgcache/master/indexedlogdatastore/0/index-node.sum
  $ hg doctor
  attempt to check and fix indexedlogdatastore ...
    Attempt to repair log "0"
    Verified 0 entries, 12 bytes in log
    Rebuilt index "node"
    Latest = 0
