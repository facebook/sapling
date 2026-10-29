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
  

Pushrebasing a stack whose root has no parent aborts cleanly:

  $ sl push -r $A --to master
  pushing rev 426bada5c675 to destination eager:$TESTTMP/server bookmark master
  edenapi: queue 1 commit for upload
  edenapi: queue 1 file for upload
  edenapi: uploaded 1 file
  edenapi: queue 1 tree for upload
  edenapi: uploaded 1 tree
  edenapi: uploaded 1 changeset
  abort: cannot pushrebase: the root commit of the stack has no parent commit
  (rebase the stack onto master first)
  [255]

Unit level: `push_rebase` itself, called with the rootless stack, aborts instead
of raising a `TypeError` from `short(None)`:

  $ sl debugshell -c '
  > from sapling import push, scmutil
  > head = scmutil.revsingle(repo, "desc(A)").node()
  > stack = repo.dageval(lambda: only([head], public()))
  > try:
  >     push.push_rebase(repo, "default", head, stack, "master", {})
  > except Exception as e:
  >     print("%s: %s" % (type(e).__name__, e))
  > '
  Abort: cannot pushrebase: the root commit of the stack has no parent commit
