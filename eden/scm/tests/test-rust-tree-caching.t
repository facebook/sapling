#require no-eden

  $ newclientrepo
  $ drawdag <<EOS
  > B
  > |
  > A
  > EOS
  $ hg go -q $A

  $ LOG=revisionstore::scmstore::tree=trace hg go -q $B 2>&1 | grep HgId
  TRACE revisionstore::scmstore::tree: Key { path: RepoPathBuf(""), hgid: HgId("41b34f08c1356f6ad068e9ab9b43d984245111aa") } found in Local
  TRACE revisionstore::scmstore::tree: Key { path: RepoPathBuf(""), hgid: HgId("eb79886383871977bccdb3000c275a279f0d4c99") } found in Local

  $ hg blackbox --no-timestamp --no-sid --pattern '{"legacy_log":{"service":"metrics"}}' | tail -1
  [legacy][metrics] {"metrics":*"treeresolver":{"cache":{"hits":6,"reqs":10}}* (glob)
