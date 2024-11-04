#require no-eden

  $ newserver server
  $ newremoterepo

  $ echo c > f
  $ hg ci -A -m 0 -q
  $ hg cat f --config scmstore.activitylog=log
  c

  $ cat log
  {"op":"FileFetch","keys":[{"path":"f","node":*}],"attrs":*,"start_millis":*,"duration_millis":*} (glob)

Use activity log to verify that replay triggers fetches
  $ hg debugscmstorereplay --path log --config scmstore.activitylog=log2
  Fetched 1 keys across 1 fetches in * (glob)
  $ cat log2
  {"op":"FileFetch","keys":[{"path":"f","node":*}],"attrs":*,"start_millis":*,"duration_millis":*} (glob)
