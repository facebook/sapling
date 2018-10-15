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
