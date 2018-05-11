# Make the test use chg if possible

if [ -x "$RUNTESTDIR/../contrib/chg/chg" ] && [ -z "$CHGHG" ]; then
  CHGHG="${HG:-hg}"
  export CHGHG
  alias hg="${CHG:-chg}"
  hg() {
    "$RUNTESTDIR/../contrib/chg/chg" "$@"
  }
fi
