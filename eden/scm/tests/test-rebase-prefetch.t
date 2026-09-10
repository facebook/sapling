#require no-eden

  $ . "$TESTDIR/library.sh"
  $ eagerepo
  $ enable rebase
  $ setconfig rebase.experimental.inmemory=True

Before rebasing starts, prefetch the manifest trees and the source, ancestor, and
destination content of every file that needs a three-way merge. The summarizer
below reports the size of every remote tree fetch request and the file content
requests, grouped by rebase phase along with how many of the requested files had
to be fetched remotely, and passes other output through:

  $ cat > $TESTTMP/summarize-fetches.py <<'PY'
  > import ast
  > import re
  > import sys
  > phase = "before rebase"
  > tree_fetches = []
  > requested = []
  > remote = 0
  > def flush():
  >     global tree_fetches, requested, remote
  >     if tree_fetches:
  >         print(f"remote tree fetch {phase}: {', '.join(tree_fetches)} keys")
  >     if requested:
  >         paths = ", ".join(sorted(set(requested)))
  >         print(f"content fetch {phase}: {paths} ({len(requested)} requests, {remote} remote)")
  >     tree_fetches = []
  >     requested = []
  >     remote = 0
  > for raw_line in sys.stdin:
  >     line = raw_line.strip()
  >     if match := re.match(r'rebasing \S+ "([^"]+)"', line):
  >         flush()
  >         phase = match.group(1)
  >         print(f"rebasing {phase}")
  >     elif '"content"' in line and (match := re.search(r"keys=(\[.*\])$", line)):
  >         requested.extend(ast.literal_eval(match.group(1)))
  >     elif match := re.search(r"revisionstore::scmstore::file::fetch: Fetching SaplingRemoteAPI - Count = (\d+)", line):
  >         remote += int(match.group(1))
  >     elif match := re.search(r"scmstore::tree::fetch: attempt to fetch (\d+) keys", line):
  >         tree_fetches.append(match.group(1))
  >     elif "revisionstore::scmstore::" not in line and "file_fetches:" not in line:
  >         print(line)
  > flush()
  > PY
  $ summarize_fetches() {
  >   SL_LOG=file_fetches=trace,revisionstore::scmstore::file=debug,revisionstore::scmstore::tree::fetch=debug "$@" > $TESTTMP/fetch-log 2>&1 || echo "exit status $?"
  >   $PYTHON $TESTTMP/summarize-fetches.py < $TESTTMP/fetch-log
  > }

  $ newserver server
  $ printf 'base 1\nbase 2\nbase 3\n' > first
  $ printf 'second base 1\nsecond base 2\nsecond base 3\n' > second
  $ echo base > unrelated
  $ sl commit -Aqm A
  $ sl bookmark main

  $ newclientrepo client server main
  $ printf 'source 1\nbase 2\nbase 3\n' > first
  $ sl commit -qm S1
  $ printf 'second source 1\nsecond base 2\nsecond base 3\n' > second
  $ sl commit -qm S2
  $ sl go -q main

  $ cd $TESTTMP/server
  $ printf 'base 1\nbase 2\ndestination 3\n' > first
  $ printf 'second base 1\nsecond base 2\nsecond destination 3\n' > second
  $ echo destination > unrelated
  $ sl commit -qm D
  $ sl bookmark destination

  $ cd $TESTTMP/client
  $ sl pull -q -B destination
  $ clearcache
  $ summarize_fetches sl rebase -s 'desc(S1)' -d destination
  remote tree fetch before rebase: 1, 1 keys
  content fetch before rebase: first, second (6 requests, 4 remote)
  rebasing S1
  merging first
  content fetch S1: first (5 requests, 0 remote)
  rebasing S2
  merging second
  content fetch S2: second (5 requests, 0 remote)

  $ sl cat -r 'desc(S2)' first
  source 1
  base 2
  destination 3
  $ sl cat -r 'desc(S2)' second
  second source 1
  second base 2
  second destination 3
  $ sl cat -r 'desc(S2)' unrelated
  destination

Source commits are not assumed to have local file content. Only files the
destination also changed need merge content, so `alone`, which only the source
commit changed, is not prefetched:

  $ newserver remote-source-server
  $ printf 'base 1\nbase 2\nbase 3\n' > shared
  $ printf 'alone 1\nalone 2\nalone 3\n' > alone
  $ sl commit -Aqm A
  $ sl bookmark main
  $ printf 'source 1\nbase 2\nbase 3\n' > shared
  $ printf 'source 1\nalone 2\nalone 3\n' > alone
  $ sl commit -qm S
  $ sl bookmark source
  $ sl go -q 'desc(A)'
  $ printf 'base 1\nbase 2\ndestination 3\n' > shared
  $ sl commit -qm D
  $ sl bookmark destination

  $ newclientrepo remote-source-client remote-source-server main
  $ sl pull -q -B source -B destination
  $ sl go -q null
  $ clearcache
  $ summarize_fetches sl rebase --keep -r source -d destination
  remote tree fetch before rebase: 3 keys
  content fetch before rebase: shared (3 requests, 3 remote)
  rebasing S
  merging shared
  content fetch S: shared (5 requests, 0 remote)

  $ sl cat -r 'desc(S)' shared
  source 1
  base 2
  destination 3
  $ sl cat -r 'desc(S)' alone
  source 1
  alone 2
  alone 3

`experimental.verify-manifest-root` restores a store lookup for each requested
manifest's root tree ahead of the batched walk:

  $ clearcache
  $ summarize_fetches sl rebase --keep -r source -d destination --config experimental.verify-manifest-root=true
  remote tree fetch before rebase: 1, 1, 1 keys
  content fetch before rebase: shared (3 requests, 3 remote)
  rebasing S
  merging shared
  content fetch S: shared (5 requests, 0 remote)

Replaying an added file does not fetch its content:

  $ newserver added-source-server
  $ echo base > base
  $ sl commit -Aqm A
  $ sl bookmark main
  $ echo added > added
  $ sl commit -Aqm S
  $ sl bookmark source
  $ sl go -q 'desc(A)'
  $ echo destination > unrelated
  $ sl commit -Aqm D
  $ sl bookmark destination

  $ newclientrepo added-source-client added-source-server main
  $ sl pull -q -B source -B destination
  $ sl go -q null
  $ clearcache
  $ summarize_fetches sl rebase --keep -r source -d destination
  remote tree fetch before rebase: 3 keys
  rebasing S

  $ sl cat -r 'desc(S)' added
  added

Deleting a file does not fetch its old content while replaying:

  $ newserver removed-source-server
  $ echo base > base
  $ echo removed > removed
  $ sl commit -Aqm A
  $ sl bookmark main
  $ sl rm removed
  $ sl commit -qm S
  $ sl bookmark source
  $ sl go -q 'desc(A)'
  $ echo destination > unrelated
  $ sl commit -Aqm D
  $ sl bookmark destination

  $ newclientrepo removed-source-client removed-source-server main
  $ sl pull -q -B source -B destination
  $ sl go -q null
  $ clearcache
  $ summarize_fetches sl rebase --keep -r source -d destination
  remote tree fetch before rebase: 3 keys
  rebasing S

Source commits with different parents each merge against their own parent, so
each root's destination is compared with that root's parent and every merge
input is prefetched before rebasing starts:

  $ newserver multi-root-server
  $ printf 'base 1\nbase 2\nbase 3\n' > first
  $ printf 'second base 1\nsecond base 2\nsecond base 3\nsecond base 4\nsecond base 5\n' > second
  $ sl commit -Aqm A
  $ sl bookmark main
  $ printf 'source 1\nbase 2\nbase 3\n' > first
  $ sl commit -qm S
  $ sl bookmark source
  $ sl go -q 'desc(A)'
  $ printf 'second b 1\nsecond base 2\nsecond base 3\nsecond base 4\nsecond base 5\n' > second
  $ sl commit -qm B
  $ printf 'second b 1\nsecond base 2\nsecond source 3\nsecond base 4\nsecond base 5\n' > second
  $ sl commit -qm T
  $ sl bookmark other-source
  $ sl go -q 'desc(A)'
  $ printf 'base 1\nbase 2\ndestination 3\n' > first
  $ printf 'second base 1\nsecond base 2\nsecond base 3\nsecond base 4\nsecond destination 5\n' > second
  $ sl commit -qm D
  $ sl bookmark destination

  $ newclientrepo multi-root-client multi-root-server main
  $ sl pull -q -B source -B other-source -B destination
  $ sl go -q null
  $ clearcache
  $ summarize_fetches sl rebase --keep -r source -r other-source -d destination
  remote tree fetch before rebase: 5 keys
  content fetch before rebase: first, second (6 requests, 6 remote)
  rebasing S
  merging first
  content fetch S: first (5 requests, 0 remote)
  rebasing T
  merging second
  content fetch T: second (5 requests, 0 remote)

  $ sl cat -r 'desc(S)' first
  source 1
  base 2
  destination 3
  $ sl cat -r 'desc(T)' second
  second base 1
  second base 2
  second source 3
  second base 4
  second destination 5

Restack uses the same stack-wide prefetch. Every manifest involved was created
locally, so clearing the cache only evicts the base content of `shared` that came
from the server:

  $ newserver restack-server
  $ printf 'base 1\nbase 2\nbase 3\n' > shared
  $ sl commit -Aqm A
  $ sl bookmark main

  $ newclientrepo restack-client restack-server main
  $ setconfig amend.autorestack=never
  $ echo b > b
  $ sl commit -Aqm B
  $ printf 'source 1\nbase 2\nbase 3\n' > shared
  $ sl commit -qm C
  $ echo source > source-only
  $ sl commit -Aqm D

  $ sl go -q 'desc(B)'
  $ printf 'base 1\nbase 2\ndestination 3\n' > shared
  $ sl amend -q
  hint[amend-restack]: descendants of b159275b8b07 are left behind - use 'sl restack' to rebase them
  hint[hint-ack]: use 'sl hint --ack amend-restack' to silence these hints
  $ clearcache
  $ summarize_fetches sl rebase --restack
  content fetch before rebase: shared (3 requests, 1 remote)
  rebasing C
  merging shared
  content fetch C: shared (5 requests, 0 remote)
  rebasing D

  $ sl cat -r 'desc(D)' shared
  source 1
  base 2
  destination 3
