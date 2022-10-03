#chg-compatible

Testing templating for rebase command

Setup

  $ setconfig workingcopy.ruststatus=False
  $ configure mutation-norecord
  $ enable rebase

  $ hg init repo
  $ cd repo
  $ for ch in a b c d; do echo foo > $ch; hg commit -Aqm "Added "$ch; done

  $ hg log -G -T "{node|short} {desc}"
  @  62615734edd5 Added d
  │
  o  28ad74487de9 Added c
  │
  o  29becc82797a Added b
  │
  o  18d04c59bb5d Added a
  
Getting the JSON output for nodechanges

  $ hg rebase -s 28ad74487de9599d00d81085be739c61fc340652 -d 18d04c59bb5d2d4090ad9a5b59bd6274adb63add -q -Tjson
  json (no-eol)

  $ hg log -G -T "{node|short} {desc}"
  @  df21b32134ba Added d
  │
  o  849767420fd5 Added c
  │
  │ o  29becc82797a Added b
  ├─╯
  o  18d04c59bb5d Added a
  
  $ hg rebase -s 29becc82797a4bc11ec8880b58eaecd2ab3e7760 -d 'max(desc(Added))' -q -T "{nodechanges|json}"
  {"29becc82797a4bc11ec8880b58eaecd2ab3e7760": ["d9d6773efc831c274eace04bc13e8e6412517139"]} (no-eol)

  $ hg log -G -T "{node|short} {desc}"
  o  d9d6773efc83 Added b
  │
  @  df21b32134ba Added d
  │
  o  849767420fd5 Added c
  │
  o  18d04c59bb5d Added a
  

  $ hg rebase -s 'max(desc(Added))' -d 849767420fd5519cf0026232411a943ed03cc9fb -q -T "{nodechanges % '{oldnode}:{newnodes % ' {newnode}'}'}"
  d9d6773efc831c274eace04bc13e8e6412517139: f48cd65c6dc3d2acb55da54402a5b029546e546f (no-eol)

  $ hg rebase -s 849767420fd5519cf0026232411a943ed03cc9fb -d 849767420fd5519cf0026232411a943ed03cc9fb -q -T "{nodechanges}"
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

