#chg-compatible
#debugruntest-compatible

  $ hg init a
  $ cd a
  $ echo AA > A
  $ hg commit -qAm "unicode quote: ’"
  $ hg log -Tjson --debug
  [
   {
    "rev": 0,
    "node": "86378053da0a233e560e42ef149017c0ae7a7e4f",
    "branch": "default",
    "phase": "draft",
    "user": "test",
    "date": [0, 0],
    "desc": "unicode quote: \xe2\x80\x99", (esc) (?)
    "desc": "unicode quote: \u2019", (?)
    "bookmarks": [],
    "parents": [],
    "manifest": "e014a281af7c5932257f42933049e389b86dc42e",
    "extra": {"branch": "default"},
    "modified": [],
    "added": ["A"],
    "removed": []
   }
  ]
