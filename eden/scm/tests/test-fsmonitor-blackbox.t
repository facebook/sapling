#require fsmonitor

  $ newrepo
  $ hg status
  $ touch x
  $ hg status
  ? x
  $ touch 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24 25
  $ hg status
  ? 1
  ? 10
  ? 11
  ? 12
  ? 13
  ? 14
  ? 15
  ? 16
  ? 17
  ? 18
  ? 19
  ? 2
  ? 20
  ? 21
  ? 22
  ? 23
  ? 24
  ? 25
  ? 3
  ? 4
  ? 5
  ? 6
  ? 7
  ? 8
  ? 9
  ? x
  $ hg blackbox --pattern '{"fsmonitor":"_"}' --no-timestamp --no-sid
  [fsmonitor] clock: "c:0:0" -> "*"; need check: [] + all files (glob)
  [fsmonitor] clock: "*" -> "*"; need check: [] + ["x"] (glob)
  [fsmonitor] clock: "*" -> "*"; need check: ["x"] + [*] and 20 entries (glob)
