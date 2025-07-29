#require ext.parallel no-eden

Test lock free operations

  $ newserver server1
  $ drawdag << 'EOS'
  > A  # bookmark master = A
  > EOS

  $ cd
  $ sl clone -q ssh://user@dummy/server1 client1

  $ drawdag --cwd ./server1 << 'EOS'
  > desc(A)-B  # bookmark master = B
  > EOS

Take a lock using 'metaedit':

  $ cd client1
  $ sl commit --config ui.allowemptycommit=1 -m x
  $ HGEDITOR='notifyevent before-edit; waitevent after-pull; echo y >' sl metaedit &>log &

Pull is not blocked by 'metaedit':

  $ waitevent before-edit
  $ sl pull
  pulling from ssh://user@dummy/server1
  imported commit graph for 1 commit (1 segment)
  $ notifyevent after-pull
  $ wait

Both pull and metaedit succeed:

  $ cat log
  $ sl log -Gr'all()' -T '{desc} {remotenames}'
  @  y
  │
  │ o  B remote/master
  ├─╯
  o  A
