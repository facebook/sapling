#chg-compatible

Using 'hgext.' prefix triggers the warning.

  $ hg init --config extensions.hgext.rebase=
  'hgext' prefix in [extensions] config section is deprecated.
  (hint: replace 'hgext.rebase' with 'rebase')

If the location of the config is printed.
Despite the warning, the extension is still loaded.

  $ setconfig extensions.hgext.rebase=
  $ hg rebase -s 'tip-tip' -d 'tip'
  'hgext' prefix in [extensions] config section is deprecated.
  (hint: replace 'hgext.rebase' with 'rebase' at $TESTTMP/.hg/hgrc:2)
  empty "source" revision set - nothing to rebase
