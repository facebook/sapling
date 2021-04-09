#chg-compatible

Emulate situations where NEED_CHECK was added to normal files and there should
be a way to remove them.

  $ newrepo
  $ drawdag << 'EOS'
  > B
  > |
  > A
  > EOS
  $ hg up $B -q

Write mtime to treestate

  $ sleep 1

  $ hg status

  $ hg debugtree list
  A: 0100644 1 + EXIST_P1 EXIST_NEXT 
  B: 0100644 1 + EXIST_P1 EXIST_NEXT 

Force the files to have NEED_CHECK bits

  $ hg debugshell -c "
  > with repo.lock(), repo.transaction('needcheck') as tr:
  >     d = repo.dirstate
  >     d.needcheck('A')
  >     d.needcheck('B')
  >     d.write(tr)
  > "
  $ hg debugtree list
  A: 0100644 1 + EXIST_P1 EXIST_NEXT NEED_CHECK 
  B: 0100644 1 + EXIST_P1 EXIST_NEXT NEED_CHECK 

Run status again. NEED_CHECK will disappear.

  $ hg status

  $ hg debugtree list
  A: 0100644 1 + EXIST_P1 EXIST_NEXT 
  B: 0100644 1 + EXIST_P1 EXIST_NEXT 

Enable sparse

  $ enable sparse
  $ hg sparse include A

When removing "B", fsmonitor+treestate will mark it as "NEED_CHECK" instead

  $ hg debugtree list
  A: 0100644 1 + EXIST_P1 EXIST_NEXT 
  B: 0100644 1 + NEED_CHECK  (fsmonitor !)

Force NEED_CHECK on files outside sparse

  $ printf B > B
  $ hg debugshell --config extensions.sparse=! -c "
  > with repo.lock(), repo.transaction('needcheck') as tr:
  >     d = repo.dirstate
  >     d.needcheck('A')
  >     d.normal('B')
  >     d.needcheck('B')
  >     d.write(tr)
  > "

Run "hg status" and NEED_CHECK can be removed:

  $ sleep 1
  $ hg status

  $ hg debugtree list
  A: 0100644 1 + EXIST_P1 EXIST_NEXT 
  B: 0100644 1 + EXIST_P1 EXIST_NEXT 
