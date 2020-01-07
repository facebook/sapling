#chg-compatible

#require serve

#if no-outer-repo

no repo

  $ hg id
  abort: there is no Mercurial repository here (.hg not found)
  [255]

#endif

create repo

  $ hg init test
  $ cd test
  $ echo a > a
  $ hg ci -Ama
  adding a

basic id usage

  $ hg id
  cb9a9f314b8b
  $ hg id --debug
  cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b
  $ hg id -q
  cb9a9f314b8b
  $ hg id -v
  cb9a9f314b8b

with options

  $ hg id -r.
  cb9a9f314b8b
  $ hg id -n
  0
  $ hg id -b
  default
  $ hg id -i
  cb9a9f314b8b
  $ hg id -n -t -b -i
  cb9a9f314b8b 0 default
  $ hg id -Tjson
  [
   {
    "bookmarks": [],
    "dirty": "",
    "id": "cb9a9f314b8b",
    "node": "ffffffffffffffffffffffffffffffffffffffff",
    "parents": [{"node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b", "rev": 0}]
   }
  ]

test template keywords and functions which require changectx:

  $ hg id -T '{rev} {node|shortest}\n'
  2147483647 ffff
  $ hg id -T '{parents % "{rev} {node|shortest} {desc}\n"}'
  0 cb9a a

with modifications

  $ echo b > a
  $ hg id -n -t -b -i
  cb9a9f314b8b+ 0+ default
  $ hg id -Tjson
  [
   {
    "bookmarks": [],
    "dirty": "+",
    "id": "cb9a9f314b8b+",
    "node": "ffffffffffffffffffffffffffffffffffffffff",
    "parents": [{"node": "cb9a9f314b8b07ba71012fcdbc544b5a4d82ff5b", "rev": 0}]
   }
  ]

other local repo

  $ cd ..
  $ hg -R test id
  cb9a9f314b8b+
#if no-outer-repo
  $ hg id test
  cb9a9f314b8b+ tip
#endif

with remote http repo

  $ cd test
  $ hg serve -p 0 --port-file $TESTTMP/.port -d --pid-file=hg.pid
  $ HGPORT1=`cat $TESTTMP/.port`
  $ cat hg.pid >> $DAEMON_PIDS
  $ hg id http://localhost:$HGPORT1/
  cb9a9f314b8b

remote with rev number?

  $ hg id -n http://localhost:$HGPORT1/
  abort: can't query remote revision number or branch
  [255]

remote with branch?

  $ hg id -b http://localhost:$HGPORT1/
  abort: can't query remote revision number or branch
  [255]

test bookmark support

  $ hg bookmark Y
  $ hg bookmark Z
  $ hg bookmarks
     Y                         0:cb9a9f314b8b
   * Z                         0:cb9a9f314b8b
  $ hg id
  cb9a9f314b8b+ Y/Z
  $ hg id --bookmarks
  Y Z

test remote identify with bookmarks

  $ hg id http://localhost:$HGPORT1/
  cb9a9f314b8b Y/Z
  $ hg id --bookmarks http://localhost:$HGPORT1/
  Y Z
  $ hg id -r . http://localhost:$HGPORT1/
  cb9a9f314b8b Y/Z
  $ hg id --bookmarks -r . http://localhost:$HGPORT1/
  Y Z

test invalid lookup

  $ hg id -r noNoNO http://localhost:$HGPORT1/
  abort: unknown revision 'noNoNO'!
  [255]

Make sure we do not obscure unknown requires file entries (issue2649)

  $ echo fake >> .hg/requires
  $ hg id
  abort: repository requires features unknown to this Mercurial: fake!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]

  $ cd ..
#if no-outer-repo
  $ hg id test
  abort: repository requires features unknown to this Mercurial: fake!
  (see https://mercurial-scm.org/wiki/MissingRequirement for more information)
  [255]
#endif
