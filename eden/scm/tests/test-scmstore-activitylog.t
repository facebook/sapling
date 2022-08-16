#chg-compatible
#debugruntest-compatible

  $ newserver server
  $ newremoterepo
  $ setconfig scmstore.enableshim=True

  $ echo c > f
  $ hg ci -A -m 0 -q
  $ hg cat f --config scmstore.activitylog=log
  c

(Fetches twice for some reason)
  $ cat log
  \{"op":"FileFetch","keys":\[\{"path":"f","node":\[([0-9]+,){19}[0-9]+\]\}\],"attrs":\{"content":true,"aux_data":false\},"start_millis":\d+,"duration_millis":\d+\} (re)
  \{"op":"FileFetch","keys":\[\{"path":"f","node":\[([0-9]+,){19}[0-9]+\]\}\],"attrs":\{"content":true,"aux_data":false\},"start_millis":\d+,"duration_millis":\d+\} (re)

Use activity log to verify that replay triggers fetches
  $ hg debugscmstorereplay --path log --config scmstore.activitylog=log2
  Fetched 2 keys across 2 fetches in * (glob)
  $ cat log2
  \{"op":"FileFetch","keys":\[\{"path":"f","node":\[([0-9]+,){19}[0-9]+\]\}\],"attrs":\{"content":true,"aux_data":false\},"start_millis":\d+,"duration_millis":\d+\} (re)
  \{"op":"FileFetch","keys":\[\{"path":"f","node":\[([0-9]+,){19}[0-9]+\]\}\],"attrs":\{"content":true,"aux_data":false\},"start_millis":\d+,"duration_millis":\d+\} (re)
