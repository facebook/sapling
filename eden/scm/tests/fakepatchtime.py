# extension to emulate invoking 'patch.internalpatch()' at the time
# specified by '[fakepatchtime] fakenow'

from __future__ import absolute_import

from sapling import extensions, patch as patchmod, util


def internalpatch(
    orig,
    ui,
    repo,
    patchobj,
    strip,
    prefix="",
    files=None,
    eolmode="strict",
    similarity=0,
):
    if files is None:
        files = set()
    r = orig(
        ui,
        repo,
        patchobj,
        strip,
        prefix=prefix,
        files=files,
        eolmode=eolmode,
        similarity=similarity,
    )

    fakenow = ui.config("fakepatchtime", "fakenow")
    if fakenow:
        fakenow = util.parsedate(fakenow)[0]
        for f in files:
            repo.wvfs.utime(f, (fakenow, fakenow))

    return r


def extsetup(ui):
    extensions.wrapfunction(patchmod, "internalpatch", internalpatch)
