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
FIXME: not correct!
  $ hg rebase -r $C -d $B
  rebasing dc0947a82db8 "C"
  note: not rebasing dc0947a82db8, its destination (rebasing onto) commit already has all its changes
  $ hg whereami
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
  112478962961147124edd43549aedd1a335e44bf


Similar, but falling back to on-disk rebase:
  $ newclientrepo
  $ drawdag <<EOS
  > B C # C/B = conflict
  > |/
  > A
  > EOS
  $ hg go -q $B
  $ hg debugsetparents $B $A
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
FIXME: not correct!
  $ hg rebase -r $C -d $B
  rebasing ce63d6ee6316 "C"
  note: not rebasing ce63d6ee6316, its destination (rebasing onto) commit already has all its changes
  $ hg whereami
  112478962961147124edd43549aedd1a335e44bf
  426bada5c67598ca65036d57d9e4b64b0c1ce7a0
