#chg-compatible
#debugruntest-compatible

  $ enable amend
  $ setconfig experimental.evolution=obsolete
  $ setconfig experimental.narrow-heads=false
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"

  $ newrepo
  $ echo "base" > base
  $ hg commit -Aqm base
  $ echo 1 > file
  $ hg commit -Aqm commit1
  $ echo 2 > file
  $ hg amend -Aqm commit1-amended
  $ tglogm --hidden
  @  f9719601f84a 'commit1-amended'
  │
  │ x  e6c779c67aa9 'commit1'  (Rewritten using amend into f9719601f84a)
  ├─╯
  o  d20a80d4def3 'base'
  
  $ hg log -r 'successors(e6c779c67aa947c951f334f4f312bd2b21d27e55)' -T '{node} {desc}\n' --hidden
  e6c779c67aa947c951f334f4f312bd2b21d27e55 commit1
  f9719601f84ab527273dc915bfb41704b111058c commit1-amended
  $ hg log -r 'predecessors(f9719601f84ab527273dc915bfb41704b111058c)' -T '{node} {desc}\n' --hidden
  e6c779c67aa947c951f334f4f312bd2b21d27e55 commit1
  f9719601f84ab527273dc915bfb41704b111058c commit1-amended

Set the phase of the obsolete commit to public, simulating the older version being landed.
  $ hg debugmakepublic e6c779c67aa947c951f334f4f312bd2b21d27e55 --hidden

The commit should no longer show up as amended.
  $ tglogm --hidden
  @  f9719601f84a 'commit1-amended'
  │
  │ o  e6c779c67aa9 'commit1'
  ├─╯
  o  d20a80d4def3 'base'
  
The predecessor and successor relationship has been removed.
  $ hg log -r 'successors(e6c779c67aa947c951f334f4f312bd2b21d27e55)' -T '{node} {desc}\n' --hidden
  e6c779c67aa947c951f334f4f312bd2b21d27e55 commit1
  $ hg log -r 'predecessors(f9719601f84ab527273dc915bfb41704b111058c)' -T '{node} {desc}\n' --hidden
  f9719601f84ab527273dc915bfb41704b111058c commit1-amended
