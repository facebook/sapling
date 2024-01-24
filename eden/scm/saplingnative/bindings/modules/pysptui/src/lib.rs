/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This software may be used and distributed according to the terms of the
 * GNU General Public License version 2.
 */

#![allow(non_camel_case_types)]

use std::cell::Cell;
use std::cell::RefCell;
use std::thread::JoinHandle;

use clidispatch::io::IO;
use cpython::*;
use cpython_ext::PyNone;
use cpython_ext::ResultPyErrExt;
use streampager::action::Action;
use streampager::action::ActionSender;
use streampager::bindings::Binding;
use streampager::bindings::Category;
use streampager::bindings::KeyCode;
use streampager::bindings::Keymap;
use streampager::bindings::Modifiers;
use streampager::config::InterfaceMode;
use streampager::control::Change;
use streampager::control::Controller;
use streampager::Pager;

pub fn init_module(py: Python, package: &str) -> PyResult<PyModule> {
    let name = [package, "sptui"].join(".");
    let m = PyModule::new(py, &name)?;
    m.add_class::<sptui>(py)?;
    m.add(py, "NONE", Modifiers::NONE.bits())?;
    m.add(py, "SHIFT", Modifiers::SHIFT.bits())?;
    m.add(py, "ALT", Modifiers::ALT.bits())?;
    m.add(py, "CTRL", Modifiers::CTRL.bits())?;
    m.add(py, "SUPER", Modifiers::SUPER.bits())?;
    Ok(m)
}

// From streampager::keymap_file::KeymapFile::parse_keycode
// TODO: export this from streampager
fn parse_keycode(ident: &str) -> Option<KeyCode> {
    use KeyCode::*;
    match ident {
        "Space" => Some(Char(' ')),
        "Cancel" => Some(Cancel),
        "Backspace" => Some(Backspace),
        "Tab" => Some(Tab),
        "Clear" => Some(Clear),
        "Enter" => Some(Enter),
        "Shift" => Some(Shift),
        "Escape" => Some(Escape),
        "Menu" => Some(Menu),
        "LeftMenu" => Some(LeftMenu),
        "RightMenu" => Some(RightMenu),
        "Pause" => Some(Pause),
        "CapsLock" => Some(CapsLock),
        "PageUp" => Some(PageUp),
        "PageDown" => Some(PageDown),
        "End" => Some(End),
        "Home" => Some(Home),
        "LeftArrow" => Some(LeftArrow),
        "RightArrow" => Some(RightArrow),
        "UpArrow" => Some(UpArrow),
        "DownArrow" => Some(DownArrow),
        "Left" => Some(LeftArrow),
        "Right" => Some(RightArrow),
        "Up" => Some(UpArrow),
        "Down" => Some(DownArrow),
        "Select" => Some(Select),
        "Print" => Some(Print),
        "Execute" => Some(Execute),
        "PrintScreen" => Some(PrintScreen),
        "Insert" => Some(Insert),
        "Delete" => Some(Delete),
        "Help" => Some(Help),
        "Applications" => Some(Applications),
        "Sleep" => Some(Sleep),
        "Numpad0" => Some(Numpad0),
        "Numpad1" => Some(Numpad1),
        "Numpad2" => Some(Numpad2),
        "Numpad3" => Some(Numpad3),
        "Numpad4" => Some(Numpad4),
        "Numpad5" => Some(Numpad5),
        "Numpad6" => Some(Numpad6),
        "Numpad7" => Some(Numpad7),
        "Numpad8" => Some(Numpad8),
        "Numpad9" => Some(Numpad9),
        "Multiply" => Some(Multiply),
        "Add" => Some(Add),
        "Separator" => Some(Separator),
        "Subtract" => Some(Subtract),
        "Decimal" => Some(Decimal),
        "Divide" => Some(Divide),
        "NumLock" => Some(NumLock),
        "ScrollLock" => Some(ScrollLock),
        "BrowserBack" => Some(BrowserBack),
        "BrowserForward" => Some(BrowserForward),
        "BrowserRefresh" => Some(BrowserRefresh),
        "BrowserStop" => Some(BrowserStop),
        "BrowserSearch" => Some(BrowserSearch),
        "BrowserFavorites" => Some(BrowserFavorites),
        "BrowserHome" => Some(BrowserHome),
        "VolumeMute" => Some(VolumeMute),
        "VolumeDown" => Some(VolumeDown),
        "VolumeUp" => Some(VolumeUp),
        "MediaNextTrack" => Some(MediaNextTrack),
        "MediaPrevTrack" => Some(MediaPrevTrack),
        "MediaStop" => Some(MediaStop),
        "MediaPlayPause" => Some(MediaPlayPause),
        "ApplicationLeftArrow" => Some(ApplicationLeftArrow),
        "ApplicationRightArrow" => Some(ApplicationRightArrow),
        "ApplicationUpArrow" => Some(ApplicationUpArrow),
        "ApplicationDownArrow" => Some(ApplicationDownArrow),
        other => {
            if other.starts_with('F') && other.chars().skip(1).all(char::is_numeric) {
                let n = other[1..].parse::<u8>().ok()?;
                Some(Function(n))
            } else {
                None
            }
        }
    }
}

fn parse_category(cat: &str) -> Option<Category> {
    match cat {
        "General" => Some(Category::General),
        "Navigation" => Some(Category::Navigation),
        "Presentation" => Some(Category::Presentation),
        "Searching" => Some(Category::Searching),
        _ => None,
    }
}

py_class!(class sptui |py| {
    data controller: RefCell<Controller>;
    data pager_handle: RefCell<Option<JoinHandle<streampager::Result<()>>>>;
    data action_sender: RefCell<ActionSender>;
    data line_count: Cell<usize>;

    // Create a new sptui instance.
    //
    // `bindings` is a list of (`binding`, `keys`) pairs.
    //
    // Each `binding` is one of:
    // * `None`, indicating the keys should be unbound.
    // * A tuple `(category, description, call)` that defines a custom binding.
    // * A string which matches an existing streampager action name.
    // * A tuple `(name, params)` which matches an existing streampager
    //   action name with parameters.
    //
    // Each `keys` is a list of (`mods`, `key`, `hidden`), where:
    // * `mods` is the modifiers (use `NONE`, `SHIFT`, `CTRL`, `ALT` and `SUPER` in
    //   this module for the values, and bitwise-or together for combinations).
    // * `key` is the key (either a single character, or the streampager key
    //   name).
    // * `hidden` is an optional boolean that hides the key from the help screen if
    //   True.
    def __new__(
        _cls,
        title: String,
        bindings: Vec<(PyObject, Vec<PyTuple>)>
    ) -> PyResult<sptui> {
        let controller = Controller::new(title);
        controller.apply_changes(vec![Change::AppendLines {
            contents: vec![]
        }]).unwrap();

        let mut pager = Pager::new_using_stdio().unwrap();

        pager.set_interface_mode(InterfaceMode::FullScreen);
        let file_index = pager.add_controlled_file(&controller).unwrap();

        let mut keymap = Keymap::default();

        for (binding, keys) in bindings {
            let binding = if binding == py.None() {
                None
            } else if let Ok(name) = binding.extract::<String>(py) {
                Some(Binding::parse(name, Vec::new()).map_err(|e| {
                    PyErr::new::<exc::ValueError, _>(py, format!("Invalid binding: {}", e))
                })?)
            } else if let Ok((name, params)) = binding.extract::<(String, Vec<String>)>(py) {
                Some(Binding::parse(name, params).map_err(|e| {
                    PyErr::new::<exc::ValueError, _>(py, format!("Invalid binding: {}", e))
                })?)
            } else if let Ok((cat, desc, callback)) = binding.extract::<(String, String, PyObject)>(py) {
                let cat = parse_category(&cat).ok_or_else(|| {
                    PyErr::new::<exc::ValueError, _>(py, format!("Invalid category '{}'", cat))
                })?;
                Some(Binding::custom(cat, desc, move |f| {
                    if f == file_index {
                        let gil = Python::acquire_gil();
                        callback.call(gil.python(), NoArgs, None).unwrap();
                    }
                }))
            } else {
                return Err(PyErr::new::<exc::ValueError, _>(
                    py, format!("Invalid binding (got {})", binding.get_type(py).name(py))
                ));
            };
            for key in keys {
                let (mods, key, hidden) = {
                    if let Ok((mods, key, hidden)) = key.as_object().extract::<(u8, String, bool)>(py) {
                        (mods, key, hidden)
                    } else if let Ok((mods, key)) = key.as_object().extract::<(u8, String)>(py) {
                        (mods, key, false)
                    } else {
                        return Err(PyErr::new::<exc::ValueError, _>(
                            py, format!("Invalid key (got {})", key.as_object().get_type(py).name(py))
                        ));
                    }
                };
                let mods = Modifiers::from_bits(mods).ok_or_else(|| {
                    PyErr::new::<exc::ValueError, _>(py, format!("Invalid modifiers '0x{:x}'", mods))
                })?;
                let keycode = if key.chars().count() == 1 {
                    KeyCode::Char(key.chars().next().unwrap())
                } else {
                    parse_keycode(&key).ok_or_else(|| {
                        PyErr::new::<exc::ValueError, _>(py, format!("Invalid key '{}'", key))
                    })?
                };
                if hidden {
                    keymap.bind_hidden(mods, keycode, binding.clone());
                } else {
                    keymap.bind(mods, keycode, binding.clone());
                }
            }
        }

        pager.set_keymap(keymap);

        let io = IO::main().unwrap();
        let (prg_read, prg_write) = pipe::pipe();
        pager.set_progress_stream(prg_read);
        io.set_progress_pipe_writer(Some(prg_write)).unwrap();

        let action_sender = pager.action_sender();

        let pager_handle = std::thread::spawn(move || {
            let result = pager.run();
            io.set_progress_pipe_writer(None).unwrap();
            result
        });

        Self::create_instance(
            py,
            RefCell::new(controller),
            RefCell::new(Some(pager_handle)),
            RefCell::new(action_sender),
            Cell::new(0),
        )
    }

    def replace_contents(&self, contents: Vec<Vec<u8>>) -> PyResult<PyNone> {
        let controller = self.controller(py).borrow();
        let lines = self.line_count(py).replace(contents.len());
        let _ = controller.apply_changes(vec![
            Change::ReplaceLines {
                range: 0..lines,
                contents,
            }
        ]);
        Ok(PyNone)
    }

    def wait(&self) -> PyResult<PyNone> {
        if let Some(handle) = self.pager_handle(py).replace(None) {
            py.allow_threads(move || handle.join()).unwrap().map_pyerr(py)?;
        }
        Ok(PyNone)
    }

    def end(&self) -> PyResult<PyNone> {
        let _ = self.action_sender(py).borrow().send(Action::Quit);
        if let Some(handle) = self.pager_handle(py).replace(None) {
            py.allow_threads(move || handle.join()).unwrap().map_pyerr(py)?;
        }
        Ok(PyNone)
    }
});
