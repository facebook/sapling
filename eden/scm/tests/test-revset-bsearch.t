#chg-compatible
#debugruntest-compatible
  $ configure modern

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1 ui.allowemptycommit=1
  $ hg commit -d '2001-6-1 UTC' -m 2001
  $ hg commit -d '2002-6-1 UTC' -m 2002
  $ hg commit -d '2003-6-1 UTC' -m 2003
  $ hg commit -d '2004-6-1 UTC' -m 2004

Binary search:

  $ hg log -r 'bsearch(date(">2001"),.)' -T '{desc}\n'
  2001
  $ hg log -r 'bsearch(date(">2002"),.)' -T '{desc}\n'
  2002
  $ hg log -r 'bsearch(date(">2003"),.)' -T '{desc}\n'
  2003
  $ hg log -r 'bsearch(date(">2004"),.)' -T '{desc}\n'
  2004

Not found:

  $ hg log -r 'bsearch(date(">2005"),.)' -T '{desc}\n'

Not found in the given range:

  $ hg log -r 'bsearch(date(">2004"),desc(2003))' -T '{desc}\n'

"top" containing more than 1 commit:

  $ hg log -r 'bsearch(date(">2004"),all())' -T '{desc}\n'
  abort: top should be a single changeset to ensure linearity
  [255]
