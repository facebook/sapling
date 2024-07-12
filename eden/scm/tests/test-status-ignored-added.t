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
FIXME: ignroed/b is shown

  $ sl status normal
  M normal/a
  A ignored/b

cp --after again should work fine:
FIXME: it shows a confusing warning

  $ sl cp --after ignored/a ignored/b
  ignored/b: not overwriting - ignored/b collides with ignored/a

