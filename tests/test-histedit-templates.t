Testing templating for histedit command

Setup

  $ cat >> $HGRCPATH <<EOF
  > [extensions]
  > histedit=
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

  $ hg histedit -Tjson --commands - 2>&1 <<EOF
  > pick 28ad74487de9 Added c
  > pick 62615734edd5 Added d
  > pick 18d04c59bb5d Added a
  > pick 29becc82797a Added b
  > EOF
  [
   {
    "nodechanges": {"18d04c59bb5d2d4090ad9a5b59bd6274adb63add": ["109f8ec895447f81b380ba8d4d8b66539ccdcb94"], "28ad74487de9599d00d81085be739c61fc340652": ["bff9e07c1807942b161dab768aa793b48e9a7f9d"], "29becc82797a4bc11ec8880b58eaecd2ab3e7760": ["f5dcf3b4db23f31f1aacf46c33d1393de303d26f"], "62615734edd52f06b6fb9c2beb429e4fe30d57b8": ["201423b441c84d9e6858daed653e0d22485c1cfa"]}
   }
  ]

  $ hg log -G -T "{rev}:{node|short} {desc}"
  @  7:f5dcf3b4db23 Added b
  |
  o  6:109f8ec89544 Added a
  |
  o  5:201423b441c8 Added d
  |
  o  4:bff9e07c1807 Added c
  
  $ hg histedit -T "{nodechanges|json}" --commands - 2>&1 <<EOF
  > pick bff9e07c1807 Added c
  > pick 201423b441c8 Added d
  > pick 109f8ec89544 Added a
  > roll f5dcf3b4db23 Added b
  > EOF
  {"109f8ec895447f81b380ba8d4d8b66539ccdcb94": ["8d01470bfeab64d3de13c49adb79d88790d38396"], "f3ec56a374bdbdf1953cacca505161442c6f3a3e": [], "f5dcf3b4db23f31f1aacf46c33d1393de303d26f": ["8d01470bfeab64d3de13c49adb79d88790d38396"]} (no-eol)
