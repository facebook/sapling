
#require eden

setup backing repo

  $ newclientrepo

test eden journal-position

  $ eden notify get-position
  *:*:0000000000000000000000000000000000000000 (glob)
  $ eden notification get-position
  *:*:0000000000000000000000000000000000000000 (glob)
