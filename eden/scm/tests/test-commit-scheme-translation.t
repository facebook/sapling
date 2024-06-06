  $ newclientrepo
  $ touch foo
  $ hg commit -Aqm foo
  $ hg push -q --to main --create
  $ hg log -r . '-T{node}\n'
  1f7b0de80e118a7ffde47b646b0d4e9ab57252fd

  $ hg debugapi -e committranslateids -i '[{"Hg": "1f7b0de80e118a7ffde47b646b0d4e9ab57252fd"}]' -i "'Hg'"
  [{"commit": {"Hg": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd")},
    "translated": {"Hg": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd")}}]
  $ hg debugapi -e committranslateids -i '[{"Hg": "1f7b0de80e118a7ffde47b646b0d4e9ab57252fd"}]' -i "'Bonsai'"
  [{"commit": {"Hg": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd")},
    "translated": {"Bonsai": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000")}}]
  $ hg debugapi -e committranslateids -i '[{"Bonsai": "1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000"}]' -i "'Bonsai'"
  [{"commit": {"Bonsai": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000")},
    "translated": {"Bonsai": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000")}}]
  $ hg debugapi -e committranslateids -i '[{"Bonsai": "1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000"}]' -i "'Hg'"
  [{"commit": {"Bonsai": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000")},
    "translated": {"Hg": bin("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd")}}]
