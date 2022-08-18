#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible
  $ configure dummyssh
  $ enable clienttelemetry
  $ setconfig clienttelemetry.announceremotehostname=true

set up the server repo
  $ hg init server
  $ cat >> server/.hg/hgrc << EOF
  > [extensions]
  > sampling=
  > [sampling]
  > filepath = $TESTTMP/sampling.txt
  > key.clienttelemetry = client
  > EOF

set up the local repo
  $ hg clone 'ssh://user@dummy/server' local -q
  $ cd local
  $ hg pull
  pulling from ssh://user@dummy/server
  connected to * (glob)
  no changes found
  $ hg pull -q
  $ hg pull --config clienttelemetry.announceremotehostname=False
  pulling from ssh://user@dummy/server
  no changes found

  $ hg pull -q --config lfs.wantslfspointers=True
  $ hg pull -q --config lfs.wantslfspointers=True --config clienttelemetryvalues.somevalue=value
  $ hg pull -q --config lfs.wantslfspointers=True \
  > --config clienttelemetryvalues.somevalue=value \
  > --config clienttelemetryvalues.anothervalue=value2
  $ hg pull -q --config infinitepush.wantsunhydratedcommits=True

check telemetry
  >>> import json, os
  >>> with open(os.path.join(os.getenv("TESTTMP"), "sampling.txt")) as f:
  ...     data = f.read()
  >>> for record in data.strip("\0").split("\0"):
  ...     parsedrecord = json.loads(record)
  ...     for key in "command", "fullcommand", "wantslfspointers", "wantsunhydratedcommits", "somevalue", "anothervalue":
  ...         if "client_%s" % key in parsedrecord["data"]:
  ...             print("%s: %s" % (key, parsedrecord["data"]["client_%s" % key]))
  command: clone
  fullcommand: clone 'ssh://user@dummy/server' local -q
  wantslfspointers: False
  wantsunhydratedcommits: False
  command: pull
  fullcommand: pull
  wantslfspointers: False
  wantsunhydratedcommits: False
  command: pull
  fullcommand: pull -q
  wantslfspointers: False
  wantsunhydratedcommits: False
  command: pull
  fullcommand: pull --config 'clienttelemetry.announceremotehostname=False'
  wantslfspointers: False
  wantsunhydratedcommits: False
  command: pull
  fullcommand: pull -q --config 'lfs.wantslfspointers=True'
  wantslfspointers: True
  wantsunhydratedcommits: False
  command: pull
  fullcommand: pull -q --config 'lfs.wantslfspointers=True' --config 'clienttelemetryvalues.somevalue=value'
  wantslfspointers: True
  wantsunhydratedcommits: False
  somevalue: value
  command: pull
  fullcommand: pull -q --config 'lfs.wantslfspointers=True' --config 'clienttelemetryvalues.somevalue=value' --config 'clienttelemetryvalues.anothervalue=value2'
  wantslfspointers: True
  wantsunhydratedcommits: False
  somevalue: value
  anothervalue: value2
  command: pull
  fullcommand: pull -q --config 'infinitepush.wantsunhydratedcommits=True'
  wantslfspointers: False
  wantsunhydratedcommits: True

check blackbox
  $ hg blackbox --pattern '{"clienttelemetry": "_"}'
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
