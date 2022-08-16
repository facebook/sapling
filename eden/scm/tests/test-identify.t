#chg-compatible
#debugruntest-compatible

#require serve

#if no-outer-repo

no repo

  $ hg id
  abort: there is no Mercurial repository here (.hg not found)
  [255]

#endif

  $ configure dummyssh

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
(The Rust layer does not special handle the wdir commit hash so shortest does
not "work" here.  In the future we want to change virtual commits handling to
use normal (non-special-cased) in-memory-only commits in the Rust DAG instead
of special casing them in various APIs (ex. partialmatch))

  $ hg id -T '{node|shortest}\n'
  ffffffffffffffffffffffffffffffffffffffff
  $ hg id -T '{parents % "{node|shortest} {desc}\n"}'
  cb9a a

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

with remote ssh repo

  $ cd test
  $ hg id ssh://user@dummy/test
  cb9a9f314b8b

remote with rev number?

  $ hg id -n ssh://user@dummy/test
  abort: can't query remote revision number or branch
  [255]

remote with branch?

  $ hg id -b ssh://user@dummy/test
  abort: can't query remote revision number or branch
  [255]

test bookmark support

  $ hg bookmark Y
  $ hg bookmark Z
  $ hg bookmarks
     Y                         cb9a9f314b8b
   * Z                         cb9a9f314b8b
  $ hg id
  cb9a9f314b8b+ Y/Z
  $ hg id --bookmarks
  Y Z

test remote identify with bookmarks

  $ hg id ssh://user@dummy/test
  cb9a9f314b8b Y/Z
  $ hg id --bookmarks ssh://user@dummy/test
  Y Z
  $ hg id -r . ssh://user@dummy/test
  cb9a9f314b8b Y/Z
  $ hg id --bookmarks -r . ssh://user@dummy/test
  Y Z

test invalid lookup

  $ hg id -r noNoNO ssh://user@dummy/test
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
