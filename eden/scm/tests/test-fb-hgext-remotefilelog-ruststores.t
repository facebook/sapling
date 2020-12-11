#chg-compatible

  $ . "$TESTDIR/library.sh"

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True remotefilelog.localdatarepack=True
  $ setconfig treemanifest.useruststore=True

  $ echo x > x
  $ hg commit -qAm x
  $ ls_l .hg/store/indexedlogdatastore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/indexedloghistorystore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  -rw-rw-r--      12 log

  $ echo y > y
  $ hg commit -qAm y
  $ ls_l .hg/store/indexedlogdatastore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/indexedloghistorystore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  -rw-rw-r--      12 log
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  -rw-rw-r--      12 log

  $ setconfig remotefilelog.write-local-to-indexedlog=True
  $ echo z > z
  $ hg commit -qAm z
  $ ls_l .hg/store/indexedlogdatastore | grep log
  -rw-rw-r--      60 log
  $ ls_l .hg/store/indexedloghistorystore | grep log
  -rw-rw-r--     127 log
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  -rw-rw-r--     192 log
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  -rw-rw-r--     124 log
