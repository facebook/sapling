#require no-eden

  $ setconfig push.edenapi=true

Server with an existing bookmark:

  $ newserver server
  $ echo foo > a
  $ sl commit -Aqm initial
  $ sl book master
  $ cd ..

Client with a draft root commit that shares no history with the server:

  $ newclientrepo client server
  $ drawdag <<'EOS'
  > A
  > EOS
  $ sl log -G -r 'all()' -T '{desc} {phase} {remotebookmarks}'
  o  A draft
  
  @  initial public remote/master
  

Pushrebasing a stack whose root has no parent crashes:

  $ sl push -r $A --to master 2>&1 | grep -E '^(TypeError|abort)'
  TypeError: 'NoneType' object is not subscriptable
