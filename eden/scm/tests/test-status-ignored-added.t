Repo setup:

  $ newrepo

  $ mkdir ignored normal
  $ touch ignored/a normal/a
  $ echo ignored/ > .gitignore

  $ sl ci -m a -A ignored/a normal/a .gitignore

  $ cp ignored/a ignored/b
  $ sl cp --after ignored/a ignored/b
  $ echo 1 >> normal/a

status DIR should not show files outside DIR:

  $ sl status normal
  M normal/a

cp --after again should work fine:

  $ sl cp --after ignored/a ignored/b

