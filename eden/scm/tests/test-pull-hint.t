#chg-compatible
  $ hg init test
  $ cd test

Test config-driven hints on `hg pull

  $ hg pull --config hint-definitions.pull:important_announcement="Important announcement text." .
  pulling from .
  no changes found
  hint[pull:important_announcement]: Important announcement text.
  hint[hint-ack]: use 'hg hint --ack pull:important_announcement' to silence these hints

