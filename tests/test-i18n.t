Translations are optional:

  $ "$TESTDIR/hghave" gettext || exit 80

Test that translations are compiled and installed correctly.

Default encoding in tests is "ascii" and the translation is encoded
using the "replace" error handler:

  $ LANGUAGE=pt_BR hg tip
  abortado: N?o h? um reposit?rio do Mercurial aqui (.hg n?o encontrado)!
  [255]

Using a more accomodating encoding:

  $ HGENCODING=UTF-8 LANGUAGE=pt_BR hg tip
  abortado: N\xc3\xa3o h\xc3\xa1 um reposit\xc3\xb3rio do Mercurial aqui (.hg n\xc3\xa3o encontrado)! (esc)
  [255]

Different encoding:

  $ HGENCODING=Latin-1 LANGUAGE=pt_BR hg tip
  abortado: N\xe3o h\xe1 um reposit\xf3rio do Mercurial aqui (.hg n\xe3o encontrado)! (esc)
  [255]
