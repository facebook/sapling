/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

use cpython::PyModule;
use cpython::PyResult;
use cpython::Python;
use cpython_ext::PyNone;
use paste::paste;

macro_rules! add_modules {
    ( $py:ident, $m:ident, [ $( $name:ident, )* ] ) => {
        let name = $m.get($py, "__name__")?.extract::<String>($py)?;
        $(
            paste! {
                $m.add($py, stringify!($name), ::[<py $name>]::init_module($py, &name)?)?;
            }
         )*
    }
}

/// Populate an existing empty module so it contains utilities.
pub(crate) fn populate_module(py: Python<'_>, module: &PyModule) -> PyResult<PyNone> {
    let m = module;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    add_modules!(
        py,
        m,
        [
            // see update_modules.sh
            // [[[cog
            // import cog, glob, os
            // for path in sorted(glob.glob('modules/py*/BUCK')):
            //     name = os.path.basename(os.path.dirname(path))
            //     cog.outl(f'{name[2:]},')
            // ]]]
            atexit,
            auth,
            blackbox,
            bytes,
            cats,
            cbor,
            cext,
            checkout,
            clientinfo,
            cliparser,
            conchparser,
            configloader,
            context,
            copytrace,
            dag,
            diffhelpers,
            dirs,
            doctor,
            drawdag,
            eagerepo,
            edenapi,
            error,
            exchange,
            fail,
            formatutil,
            fs,
            gitcompat,
            gitstore,
            hgmetrics,
            hgtime,
            hook,
            identity,
            indexedlog,
            io,
            journal,
            linelog,
            lock,
            lz4,
            manifest,
            metalog,
            modules,
            mutationstore,
            nodeipc,
            nodemap,
            pathhistory,
            pathmatcher,
            pprint,
            process,
            progress,
            refencode,
            regex,
            renderdag,
            repo,
            revisionstore,
            revlogindex,
            serde,
            sptui,
            status,
            storemodel,
            submodule,
            threading,
            tracing,
            treestate,
            version,
            vlq,
            webview,
            worker,
            workingcopy,
            workingcopyclient,
            xdiff,
            zstd,
            zstore,
            // [[[end]]]
        ]
    );

    Ok(PyNone)
}
