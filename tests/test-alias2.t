Interesting corner cases.

command name matches global flag values

  $ setconfig ui.allowemptycommit=1

  $ hg init foo
  $ hg -R foo commit -m "This is foo\n"

  $ hg init log
  $ hg -R log commit -m "This is log\n"

  $ setconfig "alias.foo=log" "alias.log=log -T {desc} -r"

  $ hg -R foo foo tip
  This is foo\n (no-eol)
  $ hg -R log foo tip
  This is log\n (no-eol)
  $ hg -R foo log tip
  This is foo\n (no-eol)
  $ hg -R log log tip
  This is log\n (no-eol)
