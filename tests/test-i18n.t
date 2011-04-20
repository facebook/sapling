Translations are optional:

  $ "$TESTDIR/hghave" gettext || exit 80

Test that translations are compiled and installed correctly.

Default encoding in tests is "ascii" and the translation is encoded
using the "replace" error handler:

  $ LANGUAGE=pt_BR hg tip
  abortado: no repository found in '$TESTTMP' (.hg not found)!
  [255]

Using a more accomodating encoding:

  $ HGENCODING=UTF-8 LANGUAGE=pt_BR hg tip
  abortado: no repository found in '$TESTTMP' (.hg not found)!
  [255]

Different encoding:

  $ HGENCODING=Latin-1 LANGUAGE=pt_BR hg tip
  abortado: no repository found in '$TESTTMP' (.hg not found)!
  [255]
