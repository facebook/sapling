#chg-compatible
#debugruntest-compatible

  $ configure modern

  $ setconfig paths.default=test:e1
  $ setconfig pull.httphashprefix=1
  $ setconfig pull.httpcommitgraph=1
  $ export LOG=exchange::httpcommitlookup=debug,pull

Disable SSH:

  $ setconfig ui.ssh=false

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ drawdag << 'EOS'
  > B C  # C/T/A=2
  > |/
  > A    # A/T/A=1
  > EOS

Push:

  $ hg push -r $C --to master --create
  pushing rev 178c10ffbc2f to destination test:e1 bookmark master
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\x17\x8c\x10\xff\xbc/\x92\xd5@|\x14G\x8a\xe9\xd9\xde\xa8\x1f#.', 'known': {'Ok': False}}
  searching for changes
  exporting bookmark master
  $ hg push -r $B --to remotebook --create
  pushing rev 99dac869f01e to destination test:e1 bookmark remotebook
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\x17\x8c\x10\xff\xbc/\x92\xd5@|\x14G\x8a\xe9\xd9\xde\xa8\x1f#.', 'known': {'Ok': True}}
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\x99\xda\xc8i\xf0\x1e\t\xfe=P\x1f\xa6E\xeaRJ\xf8\rI\x8f', 'known': {'Ok': False}}
  searching for changes
  exporting bookmark remotebook
  $ hg book --list-remote master remotebook
     master                    178c10ffbc2f92d5407c14478ae9d9dea81f232e
     remotebook                99dac869f01e09fe3d501fa645ea524af80d498f

Pull Bookmark:

  $ newremoterepo
  $ setconfig paths.default=test:e1
  $ hg debugchangelog --migrate lazy
  $ hg log -r master
  abort: unknown revision 'master'!
  [255]
  $ hg pull -B master
  pulling from test:e1
  DEBUG pull::httpbookmarks: edenapi fetched bookmarks: {'master': '178c10ffbc2f92d5407c14478ae9d9dea81f232e'}
  DEBUG pull::httpgraph: edenapi fetched graph node: 748104bd5058bf2c386d074d8dcf2704855380f6 []
  DEBUG pull::httpgraph: edenapi fetched graph node: 178c10ffbc2f92d5407c14478ae9d9dea81f232e ['748104bd5058bf2c386d074d8dcf2704855380f6']
  $ hg book --list-subscriptions
     remote/master             178c10ffbc2f

Check Graph
  $ hg log -r '178c10ffbc2f^'
  commit:      748104bd5058
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     A
  

Pull short hash with multiround sampling:
  $ drawdag << 'EOS'
  > F
  > |
  > E
  > EOS
C is known and E is unknown
  $ hg push -r $E --allow-anon
  pushing to test:e1
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\x17\x8c\x10\xff\xbc/\x92\xd5@|\x14G\x8a\xe9\xd9\xde\xa8\x1f#.', 'known': {'Ok': True}}
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\xe8\xe0\xa8\x1d\x95\x0f\xedk!}>\xc4U\xe6\x1a\xf1\xceN\xefH', 'known': {'Ok': False}}
  searching for changes
C and E are known and F is unknown
  $ hg pull -r 99dac869f01e0
  pulling from test:e1
  DEBUG pull::httpbookmarks: edenapi fetched bookmarks: {'master': '178c10ffbc2f92d5407c14478ae9d9dea81f232e'}
  DEBUG pull::fastpath: master: 178c10ffbc2f92d5407c14478ae9d9dea81f232e (unchanged)
  DEBUG pull::httphashlookup: edenapi hash lookups: ['99dac869f01e09fe3d501fa645ea524af80d498f']
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\x17\x8c\x10\xff\xbc/\x92\xd5@|\x14G\x8a\xe9\xd9\xde\xa8\x1f#.', 'known': {'Ok': True}}
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b"/'FJf\xa0\x1c\x1c\x14\xaa%yN\xf4\x10Q\x8d\xc0\x17\xaf", 'known': {'Ok': False}}
  searching for changes
  DEBUG exchange::httpcommitlookup: edenapi commitknown: {'hgid': b'\xe8\xe0\xa8\x1d\x95\x0f\xedk!}>\xc4U\xe6\x1a\xf1\xceN\xefH', 'known': {'Ok': True}}
  DEBUG pull::httpgraph: edenapi fetched graph node: 99dac869f01e09fe3d501fa645ea524af80d498f ['748104bd5058bf2c386d074d8dcf2704855380f6']
  $ hg log -r 99dac869f01e09
  commit:      99dac869f01e
  user:        test
  date:        Thu Jan 01 00:00:00 1970 +0000
  summary:     B
  
