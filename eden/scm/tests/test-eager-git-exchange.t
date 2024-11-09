#require no-eden

Test eager repo (client) to eager repo (server) in Git format.

  $ configure modern

Prepare the server repo:

  $ sl init server-git --config=format.use-eager-repo=True
  $ cd server-git
  $ drawdag << 'EOS'
  > A..E # bookmark master = E
  >      # bookmark stable = C
  > EOS

Clone to a shallow client repo:

  $ cd
  $ sl clone "test:server-git" client-git
  Cloning server-git into $TESTTMP/client-git
  Checking out 'master'
  5 files updated

  $ cd client-git

  $ cat E
  E (no-eol)

  $ sl log -Gr: -T '{desc} {remotenames}'
  @  E remote/master
  │
  o  D
  │
  o  C
  │
  o  B
  │
  o  A
