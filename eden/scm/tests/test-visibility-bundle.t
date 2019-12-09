#chg-compatible

  $ enable amend rebase remotenames
  $ setconfig experimental.evolution=
  $ setconfig experimental.narrow-heads=true
  $ setconfig visibility.enabled=true
  $ setconfig mutation.record=true mutation.enabled=true mutation.date="0 0"

Setup
  $ newrepo
  $ drawdag --print << EOS
  > E   I
  > |   |
  > D G H
  > |/|/
  > C F
  > |/
  > B
  > |
  > A
  > EOS
  426bada5c675 A
  112478962961 B
  26805aba1e60 C
  f585351a92f8 D
  9bc730a19041 E
  33441538d4aa F
  bd15fabcb808 G
  90ad90e692f7 H
  e5dcb50d5e3c I

Bundle up some of the commits and strip them from the repo.
  $ hg bundle -r "children($B)::" --base $B -f $TESTTMP/bundle.hg
  7 changesets found

  $ hg debugstrip -r "children($B)::"
  saved backup bundle to $TESTTMP/repo1/.hg/strip-backup/33441538d4aa-0bf456f0-backup.hg

The heads are changed when looking at the bundle.

  $ hg log -R $TESTTMP/bundle.hg -r "head()" -T '{node} {desc}\n'
  bd15fabcb8083473489d54a8edc58126c1facc53 G
  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  e5dcb50d5e3c977ce2bce38e15cabb0a8761c8f0 I

But this doesn't affect the real repo.

  $ hg log -r "head()" -T '{node} {desc}\n'
  112478962961147124edd43549aedd1a335e44bf B

Add some more commits to the main repo so that B is no longer a head.  Hide one of them.

  $ drawdag --print << EOS
  > J K
  > |/
  > $B
  > EOS
  112478962961 112478962961147124edd43549aedd1a335e44bf
  696cbb89a420 J
  200d7f7cf08d K

  $ hg hide $K
  hiding commit 200d7f7cf08d "K"
  1 changeset hidden

Logging the heads still works.

  $ hg log -R $TESTTMP/bundle.hg -r "head()" -T '{node} {desc}\n'
  696cbb89a420ebe8fafeb74ea2da0597a5ae2efa J
  bd15fabcb8083473489d54a8edc58126c1facc53 G
  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  e5dcb50d5e3c977ce2bce38e15cabb0a8761c8f0 I

  $ hg log -r "head()" -T '{node} {desc}\n'
  696cbb89a420ebe8fafeb74ea2da0597a5ae2efa J

Looking at the 'hidden' commits via commit hashes still works.

  $ hg log -R $TESTTMP/bundle.hg -r "head()+$K" -T '{node} {desc}\n'
  696cbb89a420ebe8fafeb74ea2da0597a5ae2efa J
  bd15fabcb8083473489d54a8edc58126c1facc53 G
  9bc730a19041f9ec7cb33c626e811aa233efb18c E
  e5dcb50d5e3c977ce2bce38e15cabb0a8761c8f0 I
  200d7f7cf08dd0246ad02cac6df356705cf0adab K
