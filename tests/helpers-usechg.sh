# Make the test use chg if possible

if [ -x $RUNTESTDIR/../contrib/chg/chg ] && [ -z "$CHGHG" ]; then
  CHGHG=${HG:-hg}
  export CHGHG
  alias hg=chg
fi
