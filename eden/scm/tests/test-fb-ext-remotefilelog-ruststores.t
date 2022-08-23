#chg-compatible
#debugruntest-compatible
  $ setconfig experimental.allowfilepeer=True

  $ . "$TESTDIR/library.sh"

  $ newserver master

  $ clone master shallow --noupdate
  $ cd shallow
  $ setconfig remotefilelog.useruststore=True
  $ setconfig treemanifest.useruststore=True

  $ echo x > x
  $ hg commit -qAm x
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *      60 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *     127 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *     101 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *     124 log (glob)

  $ echo y > y
  $ hg commit -qAm y
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *     108 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *     242 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *     237 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *     236 log (glob)

  $ echo z > z
  $ hg commit -qAm z
  $ ls_l .hg/store/indexedlogdatastore | grep log
  *     156 log (glob)
  $ ls_l .hg/store/indexedloghistorystore | grep log
  *     357 log (glob)
  $ ls_l .hg/store/manifests/indexedlogdatastore | grep log
  *     417 log (glob)
  $ ls_l .hg/store/manifests/indexedloghistorystore | grep log
  *     348 log (glob)
