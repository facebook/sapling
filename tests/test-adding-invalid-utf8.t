#require no-windows no-osx

Test that trying to add invalid utf8 files to the repository will fail.

  $ hg init
  >>> open("invalid\x80utf8", "w").write("test")
  $ hg addremove
  adding invalid\x80utf8 (esc)
  $ hg commit -m "adding a filename that is invalid utf8"
  abort: invalid file name encoding: invalid\x80utf8! (esc)
  [255]
