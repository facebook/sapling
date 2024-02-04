#debugruntest-compatible

Do not use usernames from env vars:

  $ unset HGUSER EMAIL
  $ export HGRCPATH=sys=$HGRCPATH:user=$HOME/.hgrc

Without auto username:

  $ newrepo
  $ hg commit --config ui.allowemptycommit=1 -m 1
  abort: no username supplied
  (use `hg config --user ui.username "First Last <me@example.com>"` to set your username)
  [255]

With auto username:

  $ cat > $TESTTMP/a.py << 'EOF'
  > from sapling import extensions, ui as uimod
  > def auto_username(orig, ui):
  >     return "A B <c@d.com>"
  > def uisetup(ui):
  >     extensions.wrapfunction(uimod, '_auto_username', auto_username)
  > EOF

  $ setconfig extensions.a=$TESTTMP/a.py

  $ hg commit --config ui.allowemptycommit=1 -m 1

  $ hg log -r . -T '{author}\n'
  A B <c@d.com>

The username is saved in config file:

  $ hg config ui.username
  A B <c@d.com>
