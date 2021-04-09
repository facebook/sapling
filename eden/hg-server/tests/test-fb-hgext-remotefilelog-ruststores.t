#chg-compatible

  $ . "$TESTDIR/library.sh"
  $ setconfig remotefilelog.write-hgcache-to-indexedlog=False remotefilelog.write-local-to-indexedlog=False

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True
  $ setconfig treemanifest.useruststore=True

  $ echo x > x
  $ hg commit -qAm x
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *      12 log (glob)

  $ echo y > y
  $ hg commit -qAm y
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *      12 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *      12 log (glob)

  $ setconfig remotefilelog.write-local-to-indexedlog=True
  $ echo z > z
  $ hg commit -qAm z
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *      60 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *     127 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *     192 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *     124 log (glob)
