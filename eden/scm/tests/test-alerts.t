
  $ newext crash <<EOF
  > from sapling import registrar
  > cmdtable = {}
  > command = registrar.command(cmdtable)
  > @command('crash', [])
  > def crash(ui, repo):
  >     raise Exception('something went wrong')
  > EOF

  $ newrepo

Alerts show in doctor
  $ readconfig <<'EOF'
  > [templatealias]
  > alerts="Ongoing issue\n{label(severity_color, severity)} {hyperlink(url, title)}\n{description}\n"
  > [alerts]
  > S12345.title=Test Alert
  > S12345.description=This is a test
  > S12345.severity=SEV 4
  > S12345.url=https://sapling-scm.com
  > S12345.show_in_isl=False
  > S12345.show_after_crashes_regex=None
  > EOF
  $ hg doctor 2>&1 | head -3
  Ongoing issue
  SEV 4 Test Alert
  This is a test


Alerts show in backtrace
  $ readconfig <<'EOF'
  > [templatealias]
  > alerts="Ongoing issue\n{label(severity_color, severity)} {hyperlink(url, title)}\n{description}\n"
  > [alerts]
  > S11111.title=Test Alert in Backtrace
  > S11111.description=This is a test
  > S11111.severity=SEV 4
  > S11111.url=https://sapling-scm.com
  > S11111.show-in-isl=False
  > S11111.show-after-crashes-regex=something .* wrong
  > S22222.title=This wont appear
  > S22222.description=Missing show_after_crashes_regex
  > S22222.severity=SEV 4
  > S22222.url=https://sapling-scm.com
  > S22222.show-in-isl=False
  > S33333.title=This wont appear
  > S33333.description=show-after-crashes-regex won't match
  > S33333.severity=SEV 4
  > S33333.url=https://sapling-scm.com
  > S33333.show-in-isl=False
  > S33333.show-after-crashes-regex=blahblah
  > EOF

  $ hg crash 2>&1 | head -6
  ** Sapling SCM (version *) has crashed: (glob)
  This crash may be related to an ongoing issue:
  Ongoing issue
  SEV 4 Test Alert in Backtrace
  This is a test
  Traceback (most recent call last):

Make sure we see the alert when errorredirect is configured:
  $ hg crash --config extensions.errorredirect= --config "errorredirect.script=echo redirected"
  This crash may be related to an ongoing issue:
  Ongoing issue
  SEV 4 Test Alert in Backtrace
  This is a test
  redirected
  [255]

Don't show alert with HGPLAIN:
  $ HGPLAIN=1 hg crash 2>&1 | grep SEV
  [1]
