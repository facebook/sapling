#require no-eden

  $ setconfig remotefilelog.cachelimit=50B remotefilelog.manifestlimit=50B

  $ newserver master
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 0 issues
  $ echo "a" > a ; hg add a ; hg commit -qAm a
  $ echo "b" > b ; hg add b ; hg commit -qAm b
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 0 issues
  $ cd ..
  $ clone master shallow
  $ cd shallow
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 2 issues
  'cache_size_exceeds_limit': 'cache size of * exceeds configured limit of 50. 0 files skipped.' (glob)
  'manifest_size_exceeds_limit': 'manifest cache size of * exceeds configured limit of 50. 0 files skipped.' (glob)
