  $ setconfig 'commit-scheme.bonsai.re=^[a-f0-9]{64}$'

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

Can use template func to translate:
  $ hg log -r . -T '{commitid("bonsai")}\n'
  1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000

Can use (bogus) Bonsai hash:
  $ SL_LOG=eagerepo=debug hg log -r 1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000
  DEBUG eagerepo::api: files [Bonsai(BonsaiChangesetId("1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000"))] -> Hg
  commit:      1f7b0de80e11
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     foo

  $ hg pull -r 1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000
  rewriting pull rev '1f7b0de80e118a7ffde47b646b0d4e9ab57252fd000000000000000000000000' into '1f7b0de80e118a7ffde47b646b0d4e9ab57252fd'
  pulling from test:repo1_server
