#chg-compatible

  $ configure modern

  $ setconfig paths.default=test:e1 ui.traceback=1
  $ export LOG=edenscm::mercurial::eagerpeer=trace,eagerepo=trace

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
  pushing rev dc0947a82db8 to destination test:e1 bookmark master
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   DEBUG eagerepo::api: commit_known dc0947a82db884575bb76ea10ac97b08536bfa03
   TRACE edenscm::mercurial::eagerpeer: known dc0947a82db884575bb76ea10ac97b08536bfa03: False
   DEBUG edenscm::mercurial::eagerpeer: heads = []
  searching for changes
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   TRACE edenscm::mercurial::eagerpeer: adding   blob 005d992c5dcf32993668f7cede29d296c494a5d9
   TRACE edenscm::mercurial::eagerpeer: adding   tree 41b34f08c1356f6ad068e9ab9b43d984245111aa
   TRACE edenscm::mercurial::eagerpeer: adding commit 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
   TRACE edenscm::mercurial::eagerpeer: adding   blob a2e456504a5e61f763f1a0b36a6c247c7541b2b3
   TRACE edenscm::mercurial::eagerpeer: adding   tree 5a538d6dd01b4058a549747c7947ce2dbf29f2ae
   TRACE edenscm::mercurial::eagerpeer: adding commit dc0947a82db884575bb76ea10ac97b08536bfa03
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG edenscm::mercurial::eagerpeer: pushkey bookmarks 'master': '' => 'dc0947a82db884575bb76ea10ac97b08536bfa03' (success)
  exporting bookmark master
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])

  $ hg push -r $B --allow-anon
  pushing to test:e1
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: commit_known 112478962961147124edd43549aedd1a335e44bf, dc0947a82db884575bb76ea10ac97b08536bfa03
   TRACE edenscm::mercurial::eagerpeer: known 112478962961147124edd43549aedd1a335e44bf: False
   TRACE edenscm::mercurial::eagerpeer: known dc0947a82db884575bb76ea10ac97b08536bfa03: True
  searching for changes
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   TRACE edenscm::mercurial::eagerpeer: adding   blob 35e7525ce3a48913275d7061dd9a867ffef1e34d
   TRACE edenscm::mercurial::eagerpeer: adding   tree eb79886383871977bccdb3000c275a279f0d4c99
   TRACE edenscm::mercurial::eagerpeer: adding commit 112478962961147124edd43549aedd1a335e44bf
   DEBUG edenscm::mercurial::eagerpeer: flushed
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])

Pull:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull -B master
  pulling from test:e1
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: commit_known 
   DEBUG eagerepo::api: commit_graph dc0947a82db884575bb76ea10ac97b08536bfa03 
   TRACE edenscm::mercurial::eagerpeer: graph node 426bada5c67598ca65036d57d9e4b64b0c1ce7a0 []
   TRACE edenscm::mercurial::eagerpeer: graph node dc0947a82db884575bb76ea10ac97b08536bfa03 ['426bada5c67598ca65036d57d9e4b64b0c1ce7a0']

  $ hg pull -r $B
  pulling from test:e1
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: commit_known 112478962961147124edd43549aedd1a335e44bf
   TRACE edenscm::mercurial::eagerpeer: known 112478962961147124edd43549aedd1a335e44bf: True
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: commit_known dc0947a82db884575bb76ea10ac97b08536bfa03
   TRACE edenscm::mercurial::eagerpeer: known dc0947a82db884575bb76ea10ac97b08536bfa03: True
  searching for changes
   DEBUG eagerepo::api: commit_graph 112478962961147124edd43549aedd1a335e44bf, dc0947a82db884575bb76ea10ac97b08536bfa03 dc0947a82db884575bb76ea10ac97b08536bfa03
   TRACE edenscm::mercurial::eagerpeer: graph node 112478962961147124edd43549aedd1a335e44bf ['426bada5c67598ca65036d57d9e4b64b0c1ce7a0']

  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
   DEBUG eagerepo::api: revlog_data 112478962961147124edd43549aedd1a335e44bf, dc0947a82db884575bb76ea10ac97b08536bfa03, 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
   TRACE eagerepo::api:  found: 112478962961147124edd43549aedd1a335e44bf, 94 bytes
   TRACE eagerepo::api:  found: dc0947a82db884575bb76ea10ac97b08536bfa03, 94 bytes
   TRACE eagerepo::api:  found: 426bada5c67598ca65036d57d9e4b64b0c1ce7a0, 94 bytes
  o  B
  │
  │ o  C remote/master
  ├─╯
  o  A
  
Trigger file and tree downloading:

  $ hg cat -r $B B A
   DEBUG eagerepo::api: trees eb79886383871977bccdb3000c275a279f0d4c99
   TRACE eagerepo::api:  found: eb79886383871977bccdb3000c275a279f0d4c99, 126 bytes
   DEBUG eagerepo::api: files 005d992c5dcf32993668f7cede29d296c494a5d9
   TRACE eagerepo::api:  found: 005d992c5dcf32993668f7cede29d296c494a5d9, 41 bytes
   DEBUG eagerepo::api: files 35e7525ce3a48913275d7061dd9a867ffef1e34d
   TRACE eagerepo::api:  found: 35e7525ce3a48913275d7061dd9a867ffef1e34d, 41 bytes
  AB (no-eol)

Clone:

  $ cd $TESTTMP
  $ hg clone -U --shallow test:e1 cloned
   DEBUG eagerepo::api: clone_data
  populating main commit graph
  tip commit: dc0947a82db884575bb76ea10ac97b08536bfa03
  fetching selected remote bookmarks
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])
   DEBUG eagerepo::api: bookmarks master
   DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', 'dc0947a82db884575bb76ea10ac97b08536bfa03')])

  $ cd cloned

Commit hash and message are lazy

  $ LOG=dag::protocol=debug,eagerepo=debug hg log -T '{desc} {node}\n' -r 'all()'
   DEBUG dag::protocol: resolve ids [0] remotely
   DEBUG eagerepo::api: revlog_data 426bada5c67598ca65036d57d9e4b64b0c1ce7a0, dc0947a82db884575bb76ea10ac97b08536bfa03
  A 426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  C dc0947a82db884575bb76ea10ac97b08536bfa03

Read file content:

  $ hg cat -r $C C
   DEBUG eagerepo::api: trees 5a538d6dd01b4058a549747c7947ce2dbf29f2ae
   TRACE eagerepo::api:  found: 5a538d6dd01b4058a549747c7947ce2dbf29f2ae, 126 bytes
   DEBUG eagerepo::api: files a2e456504a5e61f763f1a0b36a6c247c7541b2b3
   TRACE eagerepo::api:  found: a2e456504a5e61f763f1a0b36a6c247c7541b2b3, 41 bytes
  C (no-eol)
