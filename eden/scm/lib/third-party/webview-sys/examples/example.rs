use webview_sys as ffi;

fn main() {
    unsafe { unsafe_main() };
}

unsafe fn unsafe_main() {
    let inner = ffi::webview_new(
        b"Webview Example\0" as *const u8 as *const i8,
        b"https://react.dev\0" as *const u8 as *const i8,
        800,
        600,
        true as _,
        false as _,
        false as _,
        true as _,
        0,
        0,
        false as _,
        None,
        std::ptr::null_mut(),
    );
    loop {
        let should_exit = ffi::webview_loop(inner, 1);
        if should_exit != 0 {
            return;
        }
    }
}
