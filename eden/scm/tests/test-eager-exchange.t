#chg-compatible
#debugruntest-compatible
  $ configure modern

  $ setconfig paths.default=test:e1 ui.traceback=1
  $ export LOG=edenscm::mercurial::eagerpeer=trace,eagerepo=trace

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  >   D
  >   |
  > B C  # C/T/A=2
  > |/
  > A    # A/T/A=1
  > EOS

Push:

  $ hg push -r $C --to master --create
  pushing rev 178c10ffbc2f to destination test:e1 bookmark master
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 178c10ffbc2f92d5407c14478ae9d9dea81f232e
  DEBUG edenscm::mercurial::eagerpeer: heads = []
  searching for changes
  DEBUG eagerepo::api: commit_known 748104bd5058bf2c386d074d8dcf2704855380f6
  TRACE edenscm::mercurial::eagerpeer: known 748104bd5058bf2c386d074d8dcf2704855380f6: False
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
  TRACE edenscm::mercurial::eagerpeer: adding   blob 005d992c5dcf32993668f7cede29d296c494a5d9
  TRACE edenscm::mercurial::eagerpeer: adding   blob f976da1d0df2256cde08db84261621d5e92f77be
  TRACE edenscm::mercurial::eagerpeer: adding   tree 4c28a8a0e46c55df521ea9d682b5b6b8a91031a2
  TRACE edenscm::mercurial::eagerpeer: adding   tree 6161efd5db4f6d976d6aba647fa77c12186d3179
  TRACE edenscm::mercurial::eagerpeer: adding commit 748104bd5058bf2c386d074d8dcf2704855380f6
  TRACE edenscm::mercurial::eagerpeer: adding   blob a2e456504a5e61f763f1a0b36a6c247c7541b2b3
  TRACE edenscm::mercurial::eagerpeer: adding   blob d85e50a0f00eee8211502158e93772aec5dc3d63
  TRACE edenscm::mercurial::eagerpeer: adding   tree 319bc9670b2bff0a75b8b2dfa78867bf1f8d7aec
  TRACE edenscm::mercurial::eagerpeer: adding   tree 0ccf968573574750913fcee533939cc7ebe7327d
  TRACE edenscm::mercurial::eagerpeer: adding commit 178c10ffbc2f92d5407c14478ae9d9dea81f232e
  DEBUG edenscm::mercurial::eagerpeer: flushed
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict()
  DEBUG edenscm::mercurial::eagerpeer: flushed
  DEBUG edenscm::mercurial::eagerpeer: pushkey bookmarks 'master': '' => '178c10ffbc2f92d5407c14478ae9d9dea81f232e' (success)
  exporting bookmark master
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '178c10ffbc2f92d5407c14478ae9d9dea81f232e')])

  $ hg push -r $B --allow-anon
  pushing to test:e1
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 99dac869f01e09fe3d501fa645ea524af80d498f
  searching for changes
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '178c10ffbc2f92d5407c14478ae9d9dea81f232e')])
  TRACE edenscm::mercurial::eagerpeer: adding   blob 35e7525ce3a48913275d7061dd9a867ffef1e34d
  TRACE edenscm::mercurial::eagerpeer: adding   tree d8dc55ad2b89cdc0f1ee969e5d79bd1eaddb5b43
  TRACE edenscm::mercurial::eagerpeer: adding commit 99dac869f01e09fe3d501fa645ea524af80d498f
  DEBUG edenscm::mercurial::eagerpeer: flushed
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '178c10ffbc2f92d5407c14478ae9d9dea81f232e')])

  $ hg push -r $D --to master
  pushing rev 23d30dc6b703 to destination test:e1 bookmark master
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 23d30dc6b70380b2d939023947578ae0e0198999
  searching for changes
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '178c10ffbc2f92d5407c14478ae9d9dea81f232e')])
  TRACE edenscm::mercurial::eagerpeer: adding   blob 4eec8cfdabce9565739489483b6ad93ef7657ea9
  TRACE edenscm::mercurial::eagerpeer: adding   tree 4a38281d93dab71e695b39f85bdfbac0ce78011d
  TRACE edenscm::mercurial::eagerpeer: adding commit 23d30dc6b70380b2d939023947578ae0e0198999
  DEBUG edenscm::mercurial::eagerpeer: flushed
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '178c10ffbc2f92d5407c14478ae9d9dea81f232e')])
  DEBUG edenscm::mercurial::eagerpeer: flushed
  DEBUG edenscm::mercurial::eagerpeer: pushkey bookmarks 'master': '178c10ffbc2f92d5407c14478ae9d9dea81f232e' => '23d30dc6b70380b2d939023947578ae0e0198999' (success)
  updating bookmark master
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '23d30dc6b70380b2d939023947578ae0e0198999')])

Pull:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg pull -B master
  pulling from test:e1
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '23d30dc6b70380b2d939023947578ae0e0198999')])
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 
  DEBUG eagerepo::api: commit_graph 23d30dc6b70380b2d939023947578ae0e0198999 
  DEBUG eagerepo::api: commit_mutations 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 23d30dc6b70380b2d939023947578ae0e0198999, 748104bd5058bf2c386d074d8dcf2704855380f6

  $ hg pull -r $B
  pulling from test:e1
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 99dac869f01e09fe3d501fa645ea524af80d498f
  TRACE edenscm::mercurial::eagerpeer: known 99dac869f01e09fe3d501fa645ea524af80d498f: True
  DEBUG eagerepo::api: bookmarks master
  DEBUG edenscm::mercurial::eagerpeer: listkeyspatterns(bookmarks, ['master']) = sortdict([('master', '23d30dc6b70380b2d939023947578ae0e0198999')])
  DEBUG eagerepo::api: bookmarks master
  DEBUG eagerepo::api: commit_known 23d30dc6b70380b2d939023947578ae0e0198999
  searching for changes
  DEBUG eagerepo::api: commit_graph 99dac869f01e09fe3d501fa645ea524af80d498f 23d30dc6b70380b2d939023947578ae0e0198999
  DEBUG eagerepo::api: commit_mutations 99dac869f01e09fe3d501fa645ea524af80d498f

  $ hg log -Gr 'all()' -T '{desc} {remotenames}'
  DEBUG eagerepo::api: revlog_data 99dac869f01e09fe3d501fa645ea524af80d498f, 23d30dc6b70380b2d939023947578ae0e0198999, 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 748104bd5058bf2c386d074d8dcf2704855380f6
  TRACE eagerepo::api:  found: 99dac869f01e09fe3d501fa645ea524af80d498f, 94 bytes
  TRACE eagerepo::api:  found: 23d30dc6b70380b2d939023947578ae0e0198999, 94 bytes
  TRACE eagerepo::api:  found: 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 98 bytes
  TRACE eagerepo::api:  found: 748104bd5058bf2c386d074d8dcf2704855380f6, 98 bytes
  o  B
  │
  │ o  D remote/master
  │ │
  │ o  C
  ├─╯
  o  A
  
Trigger file and tree downloading:

  $ hg cat -r $B B A >out 2>err
  $ cat err out
  DEBUG eagerepo::api: trees d8dc55ad2b89cdc0f1ee969e5d79bd1eaddb5b43
  TRACE eagerepo::api:  found: d8dc55ad2b89cdc0f1ee969e5d79bd1eaddb5b43, 170 bytes
  DEBUG eagerepo::api: files 005d992c5dcf32993668f7cede29d296c494a5d9
  TRACE eagerepo::api:  found: 005d992c5dcf32993668f7cede29d296c494a5d9, 41 bytes
  DEBUG eagerepo::api: files 35e7525ce3a48913275d7061dd9a867ffef1e34d
  TRACE eagerepo::api:  found: 35e7525ce3a48913275d7061dd9a867ffef1e34d, 41 bytes
  AB (no-eol)

Clone (using edenapi clonedata, bypassing peer interface):

  $ cd $TESTTMP
  $ hg clone -U --shallow test:e1 --config remotefilelog.reponame=x cloned1
  fetching lazy changelog
  populating main commit graph
  DEBUG eagerepo::api: clone_data
  tip commit: 23d30dc6b70380b2d939023947578ae0e0198999
  fetching selected remote bookmarks
  DEBUG eagerepo::api: bookmarks master

Clone:

  $ cd $TESTTMP
  $ hg clone -U --shallow test:e1 cloned
  fetching lazy changelog
  populating main commit graph
  DEBUG eagerepo::api: clone_data
  tip commit: 23d30dc6b70380b2d939023947578ae0e0198999
  fetching selected remote bookmarks
  DEBUG eagerepo::api: bookmarks master

  $ cd cloned

Commit hash and message are lazy

  $ LOG=dag::protocol=debug,eagerepo=debug hg log -T '{desc} {node}\n' -r 'all()'
  DEBUG dag::protocol: resolve ids [0] remotely
  DEBUG eagerepo::api: revlog_data 748104bd5058bf2c386d074d8dcf2704855380f6, 178c10ffbc2f92d5407c14478ae9d9dea81f232e, 23d30dc6b70380b2d939023947578ae0e0198999
  A 748104bd5058bf2c386d074d8dcf2704855380f6
  C 178c10ffbc2f92d5407c14478ae9d9dea81f232e
  D 23d30dc6b70380b2d939023947578ae0e0198999

Read file content:

  $ hg cat -r $C C
  DEBUG eagerepo::api: trees 0ccf968573574750913fcee533939cc7ebe7327d
  TRACE eagerepo::api:  found: 0ccf968573574750913fcee533939cc7ebe7327d, 170 bytes
  DEBUG eagerepo::api: files a2e456504a5e61f763f1a0b36a6c247c7541b2b3
  TRACE eagerepo::api:  found: a2e456504a5e61f763f1a0b36a6c247c7541b2b3, 41 bytes
  C (no-eol)

Make a commit on tip, and amend. They do not trigger remote lookups:

  $ echo Z > Z
  $ LOG=warn hg up -q tip
  $ LOG=dag::protocol=debug,dag::cache=trace hg commit -Am Z Z
  TRACE dag::cache: cached missing ae226a63078b2a472fa38ec61318bb37e8c10bfb (definitely missing)
  DEBUG dag::cache: reusing cache (1 missing)

  $ LOG=dag::protocol=debug,dag::cache=trace hg amend -m Z1
  TRACE dag::cache: cached missing 893a1eb784b46325fb3062573ba15a22780ebe4a (definitely missing)
  DEBUG dag::cache: reusing cache (1 missing)
  DEBUG dag::cache: reusing cache (1 missing)
