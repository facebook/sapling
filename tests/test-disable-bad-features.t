Test various flags to turn off bad hg features.

  $ newrepo
  $ drawdag <<'EOS'
  > A
  > EOS
  $ hg up -Cq $A

Test disabling the `hg merge` command:
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

Test disabling the `hg branch` commands:
  $ hg branch
  default
  hint[branch-command-deprecate]: 'hg branch' command does not do what you want, and is being removed. It always prints 'default' for now. Check fburl.com/why-no-named-branches for details.
  hint[hint-ack]: use 'hg hint --ack branch-command-deprecate' to silence these hints
  $ setconfig ui.allowbranches=False
  $ hg branch foo
  abort: named branches are disabled in this repository
  (use bookmarks instead)
  [255]
  $ setconfig ui.disallowedbrancheshint="use bookmarks instead! see docs"
  $ hg branch -C
  abort: named branches are disabled in this repository
  (use bookmarks instead! see docs)
  [255]
