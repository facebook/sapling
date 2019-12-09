#chg-compatible

  $ cat >> $HGRCPATH << EOF
  > [ui]
  > ssh = $PYTHON "$TESTDIR/dummyssh"
  > [extensions]
  > clienttelemetry=
  > [clienttelemetry]
  > announceremotehostname=true
  > EOF

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

check telemetry
  >>> import json
  >>> with open("$TESTTMP/sampling.txt") as f:
  ...     data = f.read()
  >>> for record in data.strip("\0").split("\0"):
  ...     parsedrecord = json.loads(record)
  ...     for key in "command", "fullcommand":
  ...         print("%s: %s" % (key, parsedrecord["data"]["client_%s" % key]))
  command: clone
  fullcommand: clone 'ssh://user@dummy/server' local -q
  command: pull
  fullcommand: pull
  command: pull
  fullcommand: pull -q
  command: pull
  fullcommand: pull --config 'clienttelemetry.announceremotehostname=False'

check blackbox
  $ hg blackbox --pattern '{"clienttelemetry": "_"}'
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
  * [clienttelemetry] peer name: * (glob)
