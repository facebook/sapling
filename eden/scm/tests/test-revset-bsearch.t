
#require no-eden

  $ configure modern

Prepare Repo:

  $ newremoterepo
  $ setconfig paths.default=test:e1 ui.allowemptycommit=1
  $ sl commit -d '2001-6-1 UTC' -m 2001
  $ sl commit -d '2002-6-1 UTC' -m 2002
  $ sl commit -d '2003-6-1 UTC' -m 2003
  $ sl commit -d '2004-6-1 UTC' -m 2004

Binary search:

  $ sl log -r 'bsearch(date(">2001"),.)' -T '{desc}\n'
  2001
  $ sl log -r 'bsearch(date(">2002"),.)' -T '{desc}\n'
  2002
  $ sl log -r 'bsearch(date(">2003"),.)' -T '{desc}\n'
  2003
  $ sl log -r 'bsearch(date(">2004"),.)' -T '{desc}\n'
  2004

Not found:

  $ sl log -r 'bsearch(date(">2005"),.)' -T '{desc}\n'

Not found in the given range:

  $ sl log -r 'bsearch(date(">2004"),desc(2003))' -T '{desc}\n'

Multiple heads:

  $ sl log -r 'bsearch(date(">2003"),desc(2002) + desc(2004))' -T '{desc}\n'
  2003

Limit the search with roots:

  $ sl log -r 'bsearch(date(">2003"),heads=desc(2004),roots=desc(2004))' -T '{desc}\n'
  2004

Non-linear history:

  $ newrepo nonlinear
  $ drawdag <<'EOS'
  >     M
  >    / \
  >   C   D
  >   |   |
  >   B   A
  >    \ /
  >     R
  > EOS
  $ sl log -r 'bsearch(desc(A)::,desc(M))' -T '{desc}\n'
  A

Return one result when multiple roots match:

  $ newrepo multiroot
  $ drawdag <<'EOS'
  > M
  > |\
  > A B
  > EOS
  $ sl log -r 'bsearch(all(),desc(M))' -T 'result\n'
  result
