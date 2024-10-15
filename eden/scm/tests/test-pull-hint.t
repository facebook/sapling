#modern-config-incompatible

#require no-eden

#chg-compatible

  $ setconfig remotenames.selectivepull=true

  $ hg init test
  $ cd test

Test config-driven hints on `hg pull

  $ hg pull --config hint-definitions.pull:important_announcement="Important announcement text." .
  pulling from .
  hint[pull:important_announcement]: Important announcement text.
  hint[hint-ack]: use 'hg hint --ack pull:important_announcement' to silence these hints

