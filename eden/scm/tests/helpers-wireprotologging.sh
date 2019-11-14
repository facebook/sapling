capturewireprotologs() {
  cat >> "$TESTTMP/uilog.py" <<EOF
from edenscm.mercurial import extensions
from edenscm.mercurial import ui as uimod

def uisetup(ui):
    extensions.wrapfunction(uimod.ui, 'log', mylog)

def mylog(orig, self, service, *msg, **opts):
    if service in ['wireproto_requests']:
        kw = []
        for k, v in sorted(opts.iteritems()):
          if k == 'args':
            v = eval(v)
            for arg in v:
              if isinstance(arg, dict):
                v = sorted(list(arg.iteritems()))
            v = str(v)
          kw.append("%s=%s" % (k, v))
        kwstr = ", ".join(kw)
        msgstr = msg[0] % msg[1:]
        self.warn('%s: %s (%s)\n' % (service, msgstr, kwstr))
        with open('$TESTTMP/loggedrequests', 'a') as f:
          f.write('%s: %s (%s)\n' % (service, msgstr, kwstr))
    return orig(self, service, *msg, **opts)
EOF

  cat >> "$HGRCPATH" <<EOF
[extensions]
uilog=$TESTTMP/uilog.py

[wireproto]
logrequests=batch,branchmap,getbundle,hello,listkeys,lookup,between,unbundle
loggetpack=True
EOF
}
