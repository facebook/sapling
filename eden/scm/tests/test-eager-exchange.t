#chg-compatible

  $ configure modern

  $ setconfig paths.default=eager:$TESTTMP/e1 ui.traceback=1
  $ export LOG=edenscm::mercurial::eagerpeer=trace

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newrepo
  $ drawdag << 'EOS'
  > B C
  > |/
  > A
  > EOS

Push:

  $ hg push -r $C --to master --create
  pushing rev dc0947a82db8 to destination eager://$TESTTMP/e1 bookmark master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   TRACE edenscm::mercurial::eagerpeer: known dc0947a82db884575bb76ea10ac97b08536bfa03: False
   DEBUG edenscm::mercurial::eagerpeer: heads = []
  searching for changes
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   TRACE edenscm::mercurial::eagerpeer: adding   blob 005d992c5dcf32993668f7cede29d296c494a5d9
   TRACE edenscm::mercurial::eagerpeer: adding   tree 41b34f08c1356f6ad068e9ab9b43d984245111aa
   TRACE edenscm::mercurial::eagerpeer: adding commit 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
   TRACE edenscm::mercurial::eagerpeer: adding   blob a2e456504a5e61f763f1a0b36a6c247c7541b2b3
   TRACE edenscm::mercurial::eagerpeer: adding   tree 5a538d6dd01b4058a549747c7947ce2dbf29f2ae
   TRACE edenscm::mercurial::eagerpeer: adding commit dc0947a82db884575bb76ea10ac97b08536bfa03
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG edenscm::mercurial::eagerpeer: pushkey bookmarks 'master': '' => 'dc0947a82db884575bb76ea10ac97b08536bfa03' (success)
  exporting bookmark master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])

  $ hg push -r $B --allow-anon
  pushing to eager://$TESTTMP/e1
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   TRACE edenscm::mercurial::eagerpeer: known 112478962961147124edd43549aedd1a335e44bf: False
   TRACE edenscm::mercurial::eagerpeer: known dc0947a82db884575bb76ea10ac97b08536bfa03: True
  searching for changes
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   TRACE edenscm::mercurial::eagerpeer: adding   blob 35e7525ce3a48913275d7061dd9a867ffef1e34d
   TRACE edenscm::mercurial::eagerpeer: adding   tree eb79886383871977bccdb3000c275a279f0d4c99
   TRACE edenscm::mercurial::eagerpeer: adding commit 112478962961147124edd43549aedd1a335e44bf
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])

