#require docutils gettext

Error: the current ro localization has some rst defects exposed by
moving pager to core. These two warnings about references are expected
until the localization is corrected.
  $ $TESTDIR/check-gendoc ro
  checking for parse errors
  gendoc.txt:58: (WARNING/2) Inline interpreted text or phrase reference start-string without end-string.
  gendoc.txt:58: (WARNING/2) Inline interpreted text or phrase reference start-string without end-string.
