#chg-compatible

Testing templating for rebase command

Setup

  $ configure mutation-norecord
  $ enable rebase

  $ hg init repo
  $ cd repo
  $ for ch in a b c d; do echo foo > $ch; hg commit -Aqm "Added "$ch; done

  $ hg log -G -T "{rev}:{node|short} {desc}"
  @  3:62615734edd5 Added d
  |
  o  2:28ad74487de9 Added c
  |
  o  1:29becc82797a Added b
  |
  o  0:18d04c59bb5d Added a
  
Getting the JSON output for nodechanges

  $ hg rebase -s 2 -d 0 -q -Tjson
  json (no-eol)

  $ hg log -G -T "{rev}:{node|short} {desc}"
  @  5:df21b32134ba Added d
  |
  o  4:849767420fd5 Added c
  |
  | o  1:29becc82797a Added b
  |/
  o  0:18d04c59bb5d Added a
  
  $ hg rebase -s 1 -d 5 -q -T "{nodechanges|json}"
  {"29becc82797a4bc11ec8880b58eaecd2ab3e7760": ["d9d6773efc831c274eace04bc13e8e6412517139"]} (no-eol)

  $ hg log -G -T "{rev}:{node|short} {desc}"
  o  6:d9d6773efc83 Added b
  |
  @  5:df21b32134ba Added d
  |
  o  4:849767420fd5 Added c
  |
  o  0:18d04c59bb5d Added a
  

  $ hg rebase -s 6 -d 4 -q -T "{nodechanges % '{oldnode}:{newnodes % ' {newnode}'}'}"
  d9d6773efc831c274eace04bc13e8e6412517139: f48cd65c6dc3d2acb55da54402a5b029546e546f (no-eol)

  $ hg rebase -s 4 -d 4 -q -T "{nodechanges}"
  abort: source and destination form a cycle
  [255]

A more complex case, multiple replacements with a prune:

  $ testtemplate() {
  >   newrepo
  >   drawdag <<'EOS'
  >   B C D  # D/B = B
  >    \|/
  >     A
  > EOS
  >   hg rebase -q -r $B+$C -d $D -T "$1" 2>/dev/null
  > }

  $ testtemplate 'nodechanges default style:\n{nodechanges}'
  nodechanges default style:
  112478962961 -> (none)
  dc0947a82db8 -> 32d20c29f74a

  $ testtemplate '{nodechanges % "{nodechange}"}'
  112478962961 -> (none)
  dc0947a82db8 -> 32d20c29f74a

  $ testtemplate '{nodechanges % "OLD {oldnode} NEW {newnodes|nonempty}\n"}'
  OLD 112478962961147124edd43549aedd1a335e44bf NEW (none)
  OLD dc0947a82db884575bb76ea10ac97b08536bfa03 NEW 32d20c29f74a9f207416d66fbcaf72abddf1d21a

  $ testtemplate '{nodechanges % "{index} -{oldnode|short} {newnodes % '"'"'+{newnode|short}'"'"'}\n"}'
  0 -112478962961 
  1 -dc0947a82db8 +32d20c29f74a

  $ testtemplate '{nodechanges|json}'
  {"112478962961147124edd43549aedd1a335e44bf": [], "dc0947a82db884575bb76ea10ac97b08536bfa03": ["32d20c29f74a9f207416d66fbcaf72abddf1d21a"]} (no-eol)

