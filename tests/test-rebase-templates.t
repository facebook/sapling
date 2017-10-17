Testing templating for rebase command

Setup

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > rebase=
  > [experimental]
  > evolution=createmarkers
  > EOF

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
  [
   {
    "nodechanges": {"28ad74487de9599d00d81085be739c61fc340652": ["849767420fd5519cf0026232411a943ed03cc9fb"], "62615734edd52f06b6fb9c2beb429e4fe30d57b8": ["df21b32134ba85d86bca590cbe9b8b7cbc346c53"]}
   }
  ]

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
