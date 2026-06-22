  $ enable rebase
  $ setconfig agent.max-commit-fetch-count=6
  $ setconfig experimental.commit-fetch-batch-size=2
  $ export CODING_AGENT_METADATA=id=test_agent

Repo setup

  $ newclientrepo
  $ drawdag <<'EOS'
  > A01..A99
  > EOS
  $ sl go -q $A99

Requesting commits <= limit should succeed:

  $ sl log -r 'desc(A)' -T '{desc}\n' -l 6
  A01
  A02
  A03
  A04
  A05
  A06

One more than the limit triggers the abort before the over-limit commit is
yielded:

  $ sl log -r 'desc(A)' -T '{desc}\n' -l 7
  A01
  A02
  A03
  A04
  A05
  A06
  abort: revset query scanned over 6 commits
  (run 'sl help agent performance' for guidance.)
  [255]

The limit() revset should also stream the limited set lazily:

  $ sl log -r 'limit(desc(A), 7)' -T '{desc}\n'
  A01
  A02
  A03
  A04
  A05
  A06
  abort: revset query scanned over 6 commits
  (run 'sl help agent performance' for guidance.)
  [255]

Test --user:

  $ sl log --user test -T '{desc}\n' -l 2
  A99
  A98
  $ sl log --user test -T '{desc}\n' -l 7
  A99
  A98
  A97
  A96
  A95
  A94
  abort: revset query scanned over 6 commits
  (run 'sl help agent performance' for guidance.)
  [255]

Test --keyword:

  $ sl log --keyword A -T '{desc}\n' -l 2
  A99
  A98
  $ sl log --keyword A -T '{desc}\n' -l 7
  A99
  A98
  A97
  A96
  A95
  A94
  abort: revset query scanned over 6 commits
  (run 'sl help agent performance' for guidance.)
  [255]

Test --date:
  $ sl log --date '1970-01-01' -T '{desc}\n' -l 2
  A99
  A98
  $ sl log --date '1970-01-01' -T '{desc}\n' -l 7
  A99
  A98
  A97
  A96
  A95
  A94
  abort: revset query scanned over 6 commits
  (run 'sl help agent performance' for guidance.)
  [255]

Test prefetches is triggered for addset/filteredset, should abort when the max
fetch count is reached:

  $ sl log -r 'desc(A0) | desc(A5)' -T "{desc}\n" --config agent.max-commit-fetch-count=20
  A01
  A02
  A03
  A04
  A05
  A06
  A07
  A08
  abort: revset query scanned over 20 commits
  (run 'sl help agent performance' for guidance.)
  [255]

Disable the detection:

  $ sl log -r 'desc(A)' -T '{desc}\n' -l 8 --config agent.max-commit-fetch-count=0
  A01
  A02
  A03
  A04
  A05
  A06
  A07
  A08
