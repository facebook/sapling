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

  $ hg debugstate -v
  * A EXIST_P1 EXIST_NEXT (glob)
  * B EXIST_P1 EXIST_NEXT (glob)

Force the files to have NEED_CHECK bits

  $ hg debugshell -c "
  > with repo.lock(), repo.transaction('needcheck') as tr:
  >     d = repo.dirstate
  >     d.needcheck('A')
  >     d.needcheck('B')
  >     d.write(tr)
  > "
  $ hg debugstate -v
  * A EXIST_P1 EXIST_NEXT NEED_CHECK (glob)
  * B EXIST_P1 EXIST_NEXT NEED_CHECK (glob)

Run status again

  $ hg status

  $ hg debugstate -v
  * A EXIST_P1 EXIST_NEXT NEED_CHECK (glob)
  * B EXIST_P1 EXIST_NEXT NEED_CHECK (glob)

XXX: NEED_CHECK are not removed, although we know the files are clean.
