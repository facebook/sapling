#chg-compatible
#debugruntest-compatible
#inprocess-hg-incompatible

  $ setconfig workingcopy.ruststatus=False

Using 'ext.' prefix triggers the warning.

  $ hg init --config extensions.ext.rebase=
  'ext' prefix in [extensions] config section is deprecated.
  (hint: replace 'ext.rebase' with 'rebase')

If the location of the config is printed.
Despite the warning, the extension is still loaded.

  $ setconfig extensions.ext.rebase=
  $ hg rebase -s 'tip-tip' -d 'tip'
  'ext' prefix in [extensions] config section is deprecated.
  (hint: replace 'ext.rebase' with 'rebase' at $TESTTMP/.hg/hgrc:2)
  empty "source" revision set - nothing to rebase
