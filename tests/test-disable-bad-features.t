Test various flags to turn off bad hg features.

  $ newrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg up -Cq $A

Test disabling the merge command:
  $ hg merge
  abort: nothing to merge
  [255]
  $ setconfig ui.allowmerge=False
  $ hg merge
  abort: merging is not supported for this repository
  (use rebase instead)
  [255]

Test disabling the `hg tag` command:
  $ hg tag foo
  $ hg tags
  tip                                1:9b0f5d3c138d
  foo                                0:426bada5c675
  $ setconfig ui.allowtags=False
  $ hg tag foo2
  abort: new tags are disabled in this repository
  [255]
  $ hg tags
  abort: tags are disabled in this repository
  [255]
