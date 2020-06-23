#require fsmonitor

  $ setconfig format.dirstate=2
  $ newrepo
  $ touch x
  $ hg status
  ? x
  $ hg debugshell --command 'print(repo.dirstate.getclock())'
  c:* (glob)

Change the clock to an invalid value

  $ hg debugshell --command 'with repo.wlock(), repo.lock(), repo.transaction("dirstate") as tr: repo.dirstate.setclock("c:11111:22222"); repo.dirstate.write(tr)'
  $ hg debugshell --command 'print(repo.dirstate.getclock())'
  c:11111:22222

Run "hg status" again. A new clock value will be written even if no files are changed

  $ hg status
  ? x
  $ hg debugshell --command 'print(repo.dirstate.getclock() != "c:11111:22222")'
  True
