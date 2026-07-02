#modern-config-incompatible

#require no-eden



  $ sl init test
  $ cd test

Test config-driven hints on `sl pull

  $ sl pull --config hint-definitions.pull:important_announcement="Important announcement text." .
  pulling from .
  hint[pull:important_announcement]: Important announcement text.
  hint[hint-ack]: use 'sl hint --ack pull:important_announcement' to silence these hints

