Interesting corner cases.

command name matches global flag values

  $ setconfig ui.allowemptycommit=1

  $ hg init foo
  $ hg -R foo commit -m "This is foo\n"

  $ hg init log
  $ hg -R log commit -m "This is log\n"

  $ setconfig "alias.foo=log" "alias.log=log -T {desc} -r"

FIXME: "abort" output below is incorrect.
  $ hg -R foo foo tip
  abort: option -R has to be separated from other options (e.g. not -qR) and --repository may only be abbreviated as --repo!
  [255]
  $ hg -R log foo tip
  This is log\n (no-eol)
  $ hg -R foo log tip
  This is foo\n (no-eol)
  $ hg -R log log tip
  abort: unknown revision 'log'!
  (if log is a remote bookmark or commit, try to 'hg pull' it first)
  [255]
