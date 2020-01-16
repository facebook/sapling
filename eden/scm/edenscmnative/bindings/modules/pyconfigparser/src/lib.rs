/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::RefCell;

use cpython::exc::UnicodeDecodeError;
use cpython::*;

use configparser::{
    config::{ConfigSet, Options},
    hg::{parse_list, ConfigSetHgExt, OptionsHgExt, HGRCPATH},
};
use cpython_ext::Bytes;
use encoding::{local_bytes_to_path, path_to_local_bytes};

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "configparser"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<config>(py)?;
    m.add(py, "parselist", py_fn!(py, parselist(value: String)))?;
    Ok(m)
}

py_class!(pub class config |py| {
    data cfg: RefCell<ConfigSet>;

    def __new__(_cls) -> PyResult<config> {
        config::create_instance(py, RefCell::new(ConfigSet::new()))
    }

    def clone(&self) -> PyResult<config> {
        let cfg = self.cfg(py).borrow();
        config::create_instance(py, RefCell::new(cfg.clone()))
    }

    def readpath(
        &self,
        path: &PyBytes,
        source: String,
        sections: Option<Vec<String>>,
        remap: Option<Vec<(String, String)>>,
        readonly_items: Option<Vec<(String, String)>>
    ) -> PyResult<Vec<Bytes>> {
        let path = local_bytes_to_path(path.data(py)).map_err(|_| encoding_error(py, path))?;
        let mut cfg = self.cfg(py).borrow_mut();

        let mut opts = Options::new().source(source).process_hgplain();
        if let Some(sections) = sections {
            opts = opts.whitelist_sections(sections);
        }
        if let Some(remap) = remap {
            let map = remap.into_iter().collect();
            opts = opts.remap_sections(map);
        }
        if let Some(readonly_items) = readonly_items {
            opts = opts.readonly_items(readonly_items);
        }

        let errors = cfg.load_path(path, &opts);
        Ok(errors_to_bytes_vec(errors))
    }

    def parse(&self, content: String, source: String) -> PyResult<Vec<Bytes>> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.into();
        let errors = cfg.parse(content, &opts);
        Ok(errors_to_bytes_vec(errors))
    }

    def get(&self, section: String, name: String) -> PyResult<Option<Bytes>> {
        let cfg = self.cfg(py).borrow();

        Ok(cfg.get(section, name).map(|v| v.to_vec().into()))
    }

    def sources(
        &self, section: String, name: String
    ) -> PyResult<Vec<(Option<Bytes>, Option<(PyBytes, usize, usize, usize)>, Bytes)>> {
        // Return [(value, file_source, source)]
        // file_source is a tuple of (file_path, byte_start, byte_end, line)
        let cfg = self.cfg(py).borrow();
        let sources = cfg.get_sources(section, name);
        let mut result = Vec::with_capacity(sources.len());
        for source in sources {
            let value = source.value().as_ref().map(|v| v.to_vec().into());
            let file = source.location().map(|(path, range)| {
                // Calculate the line number - count "\n" till range.start
                let file = source.file_content().unwrap();
                let line = 1 + file.slice(0, range.start).iter().filter(|ch| **ch == b'\n').count();

                let bytes = path_to_local_bytes(&path).unwrap();
                let pypath = if bytes.is_empty() {
                    PyBytes::new(py, b"<builtin>")
                } else {
                    let path = util::path::normalize_for_display_bytes(&bytes);
                    PyBytes::new(py, path)
                };
                (pypath, range.start, range.end, line)
            });
            let source = source.source().clone().to_vec().into();
            result.push((value.into(), file, source));
        }
        Ok(result)
    }

    def set(
        &self, section: String, name: String, value: Option<String>, source: String
    ) -> PyResult<PyObject> {
        let mut cfg = self.cfg(py).borrow_mut();
        let opts = source.into();
        cfg.set(section, name, value, &opts);
        Ok(py.None())
    }

    def sections(&self) -> PyResult<Vec<Bytes>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.sections().iter().map(|s| s.to_vec().into()).collect())
    }

    def names(&self, section: String) -> PyResult<Vec<Bytes>> {
        let cfg = self.cfg(py).borrow();
        Ok(cfg.keys(section).iter().map(|s| s.to_vec().into()).collect())
    }

    @staticmethod
    def load() -> PyResult<(config, Vec<Bytes>)> {
        let mut cfg = ConfigSet::new();
        let mut errors = Vec::new();
        // Only load builtin configs if HGRCPATH is not set.
        if std::env::var(HGRCPATH).is_err() {
            cfg.parse(MERGE_TOOLS_CONFIG, &"merge-tools.rc".into());
        }
        errors.append(&mut cfg.load_system());
        errors.append(&mut cfg.load_user());
        let errors = errors_to_bytes_vec(errors);
        config::create_instance(py, RefCell::new(cfg)).map(|cfg| (cfg, errors))
    }
});

impl config {
    pub fn get_cfg(&self, py: Python) -> ConfigSet {
        self.cfg(py).clone().into_inner()
    }
}

fn parselist(_py: Python, value: String) -> PyResult<Vec<Bytes>> {
    Ok(parse_list(value)
        .iter()
        .map(|v| v.to_vec().into())
        .collect())
}

fn encoding_error(py: Python, input: &PyBytes) -> PyErr {
    use std::ffi::CStr;
    let utf8 = CStr::from_bytes_with_nul(b"utf8\0").unwrap();
    let reason = CStr::from_bytes_with_nul(b"invalid encoding\0").unwrap();
    let input = input.data(py);
    let err = UnicodeDecodeError::new(py, utf8, input, 0..input.len(), reason).unwrap();
    PyErr::from_instance(py, err)
}

fn errors_to_bytes_vec(errors: Vec<configparser::error::Error>) -> Vec<Bytes> {
    errors
        .into_iter()
        .map(|err| format!("{}", err).into())
        .collect()
}

const MERGE_TOOLS_CONFIG: &str = r#"# Some default global settings for common merge tools

[merge-tools]
kdiff3.args=--auto --L1 base --L2 local --L3 other $base $local $other -o $output
kdiff3.regkey=Software\KDiff3
kdiff3.regkeyalt=Software\Wow6432Node\KDiff3
kdiff3.regappend=\kdiff3.exe
kdiff3.fixeol=True
kdiff3.gui=True
kdiff3.diffargs=--L1 $plabel1 --L2 $clabel $parent $child

gvimdiff.args=--nofork -d -g -O $local $other $base
gvimdiff.regkey=Software\Vim\GVim
gvimdiff.regkeyalt=Software\Wow6432Node\Vim\GVim
gvimdiff.regname=path
gvimdiff.priority=-9
gvimdiff.diffargs=--nofork -d -g -O $parent $child

vimdiff.args=$local $other $base -c 'redraw | echomsg "hg merge conflict, type \":cq\" to abort vimdiff"'
vimdiff.check=changed
vimdiff.priority=-10

merge.check=conflicts
merge.priority=-100

gpyfm.gui=True

meld.gui=True
meld.args=--label='local' $local --label='merged' $base --label='other' $other -o $output
meld.check=changed
meld.diffargs=-a --label=$plabel1 $parent --label=$clabel $child

tkdiff.args=$local $other -a $base -o $output
tkdiff.gui=True
tkdiff.priority=-8
tkdiff.diffargs=-L $plabel1 $parent -L $clabel $child

xxdiff.args=--show-merged-pane --exit-with-merge-status --title1 local --title2 base --title3 other --merged-filename $output --merge $local $base $other
xxdiff.gui=True
xxdiff.priority=-8
xxdiff.diffargs=--title1 $plabel1 $parent --title2 $clabel $child

diffmerge.regkey=Software\SourceGear\SourceGear DiffMerge\
diffmerge.regkeyalt=Software\Wow6432Node\SourceGear\SourceGear DiffMerge\
diffmerge.regname=Location
diffmerge.priority=-7
diffmerge.args=-nosplash -merge -title1=local -title2=merged -title3=other $local $base $other -result=$output
diffmerge.check=changed
diffmerge.gui=True
diffmerge.diffargs=--nosplash --title1=$plabel1 --title2=$clabel $parent $child

p4merge.args=$base $local $other $output
p4merge.regkey=Software\Perforce\Environment
p4merge.regkeyalt=Software\Wow6432Node\Perforce\Environment
p4merge.regname=P4INSTROOT
p4merge.regappend=\p4merge.exe
p4merge.gui=True
p4merge.priority=-8
p4merge.diffargs=$parent $child

p4mergeosx.executable = /Applications/p4merge.app/Contents/MacOS/p4merge
p4mergeosx.args = $base $local $other $output
p4mergeosx.gui = True
p4mergeosx.priority=-8
p4mergeosx.diffargs=$parent $child

tortoisemerge.args=/base:$base /mine:$local /theirs:$other /merged:$output
tortoisemerge.regkey=Software\TortoiseSVN
tortoisemerge.regkeyalt=Software\Wow6432Node\TortoiseSVN
tortoisemerge.check=changed
tortoisemerge.gui=True
tortoisemerge.priority=-8
tortoisemerge.diffargs=/base:$parent /mine:$child /basename:$plabel1 /minename:$clabel

ecmerge.args=$base $local $other --mode=merge3 --title0=base --title1=local --title2=other --to=$output
ecmerge.regkey=Software\Elli\xc3\xa9 Computing\Merge
ecmerge.regkeyalt=Software\Wow6432Node\Elli\xc3\xa9 Computing\Merge
ecmerge.gui=True
ecmerge.diffargs=$parent $child --mode=diff2 --title1=$plabel1 --title2=$clabel

# editmerge is a small script shipped in contrib.
# It needs this config otherwise it behaves the same as internal:local
editmerge.args=$output
editmerge.check=changed
editmerge.premerge=keep

filemerge.executable=/Developer/Applications/Utilities/FileMerge.app/Contents/MacOS/FileMerge
filemerge.args=-left $other -right $local -ancestor $base -merge $output
filemerge.gui=True

filemergexcode.executable=/Applications/Xcode.app/Contents/Applications/FileMerge.app/Contents/MacOS/FileMerge
filemergexcode.args=-left $other -right $local -ancestor $base -merge $output
filemergexcode.gui=True

; Windows version of Beyond Compare
beyondcompare3.args=$local $other $base $output /ro /lefttitle=local /centertitle=base /righttitle=other /automerge /reviewconflicts /solo
beyondcompare3.regkey=Software\Scooter Software\Beyond Compare 3
beyondcompare3.regname=ExePath
beyondcompare3.gui=True
beyondcompare3.priority=-2
beyondcompare3.diffargs=/lro /lefttitle=$plabel1 /righttitle=$clabel /solo /expandall $parent $child

; Linux version of Beyond Compare
bcompare.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcompare.gui=True
bcompare.priority=-1
bcompare.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

; OS X version of Beyond Compare
bcomposx.executable = /Applications/Beyond Compare.app/Contents/MacOS/bcomp
bcomposx.args=$local $other $base -mergeoutput=$output -ro -lefttitle=parent1 -centertitle=base -righttitle=parent2 -outputtitle=merged -automerge -reviewconflicts -solo
bcomposx.gui=True
bcomposx.priority=-1
bcomposx.diffargs=-lro -lefttitle=$plabel1 -righttitle=$clabel -solo -expandall $parent $child

winmerge.args=/e /x /wl /ub /dl other /dr local $other $local $output
winmerge.regkey=Software\Thingamahoochie\WinMerge
winmerge.regkeyalt=Software\Wow6432Node\Thingamahoochie\WinMerge\
winmerge.regname=Executable
winmerge.check=changed
winmerge.gui=True
winmerge.priority=-10
winmerge.diffargs=/r /e /x /ub /wl /dl $plabel1 /dr $clabel $parent $child

araxis.regkey=SOFTWARE\Classes\TypeLib\{46799e0a-7bd1-4330-911c-9660bb964ea2}\7.0\HELPDIR
araxis.regappend=\ConsoleCompare.exe
araxis.priority=-2
araxis.args=/3 /a2 /wait /merge /title1:"Other" /title2:"Base" /title3:"Local :"$local $other $base $local $output
araxis.checkconflict=True
araxis.binary=True
araxis.gui=True
araxis.diffargs=/2 /wait /title1:$plabel1 /title2:$clabel $parent $child

diffuse.priority=-3
diffuse.args=$local $base $other
diffuse.gui=True
diffuse.diffargs=$parent $child

UltraCompare.regkey=Software\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.regkeyalt=Software\Wow6432Node\Microsoft\Windows\CurrentVersion\App Paths\UC.exe
UltraCompare.args = $base $local $other -title1 base -title3 other
UltraCompare.priority = -2
UltraCompare.gui = True
UltraCompare.binary = True
UltraCompare.check = conflicts,changed
UltraCompare.diffargs=$child $parent -title1 $clabel -title2 $plabel1
"#;
