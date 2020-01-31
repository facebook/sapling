#chg-compatible

Test the flag to reuse another commit's message (-M):

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up -Cq $B
  $ touch afile
  $ hg add afile
  $ hg commit -M $B
  $ tglog
  @  2: 1c3d011e7c74 'B'
  |
  o  1: 112478962961 'B'
  |
  o  0: 426bada5c675 'A'
  
Ensure it's incompatible with other flags:
  $ echo 'canada rocks, eh?' > afile
  $ hg commit -M . -m 'this command will fail'
  abort: --reuse-message and --message are mutually exclusive
  [255]
  $ echo 'Super duper commit message' > ../commitmessagefile
  $ hg commit -M . -l ../commitmessagefile
  abort: --reuse-message and --logfile are mutually exclusive
  [255]
Ensure it supports nonexistant revisions:

  $ hg commit -M thisrevsetdoesnotexist
  abort: unknown revision 'thisrevsetdoesnotexist'!
  (if thisrevsetdoesnotexist is a remote bookmark or commit, try to 'hg pull' it first)
  [255]

Ensure it populates the message editor:

  $ HGEDITOR=cat hg commit -M . -e
  B
  
  
  HG: Enter commit message.  Lines beginning with 'HG:' are removed.
  HG: Leave message empty to abort commit.
  HG: --
  HG: user: test
  HG: branch 'default'
  HG: changed afile
