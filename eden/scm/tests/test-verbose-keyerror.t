#chg-compatible
#require py2

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS

  $ rm .hg/store/data/_b.i
  $ hg log -r 'desc(B)' -p
  abort: failed to fetch B at commit 112478962961147124edd43549aedd1a335e44bf (draft)
  (stack:
    112478962961 B
    426bada5c675 A)
  data/B.i@35e7525ce3a4: no match found!
  [255]
