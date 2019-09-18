// Copyright Facebook, Inc. 2017
//
// This software may be used and distributed according to the terms of the
// GNU General Public License version 2 or any later version.

#![allow(non_camel_case_types)]

use cpython::py_module_initializer;

py_module_initializer!(bindings, initbindings, PyInit_bindings, |py, m| {
    env_logger::init();

    let name = m.get(py, "__name__")?.extract::<String>(py)?;
    m.add(py, "__doc__", "Mercurial Rust Bindings")?;
    m.add(py, "blackbox", pyblackbox::init_module(py, &name)?)?;
    m.add(
        py,
        "bookmarkstore",
        pybookmarkstore::init_module(py, &name)?,
    )?;
    m.add(py, "cliparser", pycliparser::init_module(py, &name)?)?;
    m.add(py, "commands", pycommands::init_module(py, &name)?)?;
    m.add(py, "configparser", pyconfigparser::init_module(py, &name)?)?;
    m.add(py, "dag", pydag::init_module(py, &name)?)?;
    m.add(py, "edenapi", pyedenapi::init_module(py, &name)?)?;
    m.add(py, "lz4", pylz4::init_module(py, &name)?)?;
    m.add(py, "manifest", pymanifest::init_module(py, &name)?)?;
    m.add(
        py,
        "mutationstore",
        pymutationstore::init_module(py, &name)?,
    )?;
    m.add(py, "nodemap", pynodemap::init_module(py, &name)?)?;
    m.add(py, "pathmatcher", pypathmatcher::init_module(py, &name)?)?;
    m.add(
        py,
        "revisionstore",
        pyrevisionstore::init_module(py, &name)?,
    )?;
    m.add(py, "revlogindex", pyrevlogindex::init_module(py, &name)?)?;
    m.add(py, "stackdesc", pystackdesc::init_module(py, &name)?)?;
    m.add(py, "treestate", pytreestate::init_module(py, &name)?)?;
    m.add(py, "vlq", pyvlq::init_module(py, &name)?)?;
    m.add(py, "zstd", pyzstd::init_module(py, &name)?)?;
    Ok(())
});
