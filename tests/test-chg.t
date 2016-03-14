init repo

  $ hg init foo
  $ cd foo

ill-formed config

  $ hg status
  $ echo '=brokenconfig' >> $HGRCPATH
  $ hg status
  hg: parse error at * (glob)
  [255]
