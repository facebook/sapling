  $ configure modern

  $ newserver master
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > cachelimit = 0B
  > manifestlimit = 0B
  > EOF
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 0 issues
  $ echo "a" > a ; hg add a ; hg commit -qAm a
  $ echo "b" > b ; hg add b ; hg commit -qAm b
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 0 issues
  $ cd ..
  $ clone master shallow
  $ cd shallow
  $ cat >> .hg/hgrc <<EOF
  > [remotefilelog]
  > cachelimit = 0B
  > manifestlimit = 0B
  > EOF
  $ hg debugdetectissues
  ran issue detector 'cachesizeexceedslimit', found 2 issues
  'cache_size_exceeds_limit': 'cache size of 2610 exceeds configured limit of 0. 0 files skipped.'
  'manifest_size_exceeds_limit': 'manifest cache size of 2426 exceeds configured limit of 0. 0 files skipped.'
