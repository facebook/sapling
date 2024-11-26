#require no-eden

  $ enable rebase undo
  $ setconfig rebase.experimental.inmemory=true
  $ newclientrepo
  $ drawdag <<EOS
  > B C
  > |/
  > A
  > EOS
  $ hg go -q $A
  $ hg debugsetparents $A $B
  $ hg whereami
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf
Rebase is in-memory - works okay and leaves us in merge state:
  $ hg rebase -r $C -d $B
  rebasing dc0947a82db8 "C"
  $ hg whereami
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf


Falling back to on-disk rebase:
  $ newclientrepo
  $ drawdag <<EOS
  > B C # C/B = conflict
  > |/
  > A
  > EOS
Start with rebase dest checked out:
  $ hg go -q $B
  $ hg debugsetparents $B $A
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
Rebase errors out after transitioning to on-disk merge:
  $ hg rebase -r $C -d $B
  rebasing ce63d6ee6316 "C"
  merging B
  hit merge conflicts (in B); switching to on-disk merge
  abort: outstanding uncommitted merge
  [255]
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0

Start with rebase dest _not_ checked out:
  $ hg go -q $A
  $ hg debugsetparents $B $A
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
Rebase errors out after transitioning to on-disk merge:
  $ hg rebase -r $C -d $B
  rebasing ce63d6ee6316 "C"
  merging B
  hit merge conflicts (in B); switching to on-disk merge
  abort: outstanding uncommitted merge
  [255]
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
