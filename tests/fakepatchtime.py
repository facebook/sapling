# extension to emulate invoking 'patch.internalpatch()' at the time
# specified by '[fakepatchtime] fakenow'

from mercurial import extensions, patch as patchmod, util

def internalpatch(orig, ui, repo, patchobj, strip,
                  prefix='', files=None,
                  eolmode='strict', similarity=0):
    if files is None:
        files = set()
    r = orig(ui, repo, patchobj, strip,
             prefix=prefix, files=files,
             eolmode=eolmode, similarity=similarity)

    fakenow = ui.config('fakepatchtime', 'fakenow')
    if fakenow:
        # parsing 'fakenow' in YYYYmmddHHMM format makes comparison between
        # 'fakenow' value and 'touch -t YYYYmmddHHMM' argument easy
        fakenow = util.parsedate(fakenow, ['%Y%m%d%H%M'])[0]
        for f in files:
            repo.wvfs.utime(f, (fakenow, fakenow))

    return r

def extsetup(ui):
    extensions.wrapfunction(patchmod, 'internalpatch', internalpatch)
