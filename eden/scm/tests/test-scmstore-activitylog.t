#chg-compatible

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
