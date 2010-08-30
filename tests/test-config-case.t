  $ echo '[Section]' >> $HGRCPATH
  $ echo 'KeY = Case Sensitive' >> $HGRCPATH
  $ echo 'key = lower case' >> $HGRCPATH

  $ hg showconfig Section
  Section.KeY=Case Sensitive
  Section.key=lower case

