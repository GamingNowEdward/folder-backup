/// Windows DWM Acrylic 背景（DWMWA_SYSTEMBACKDROP_TYPE = DWMSBT_ACRYLIC）。
#[cfg(target_os = "windows")]
pub fn enable(hwnd: isize) {
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMSBT_ACRYLIC: i32 = 3;

    unsafe {
        let backdrop = DWMSBT_ACRYLIC;
        windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute(
            hwnd as *mut core::ffi::c_void,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const i32 as *const core::ffi::c_void,
            core::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(target_os = "windows"))]
pub fn enable(_hwnd: isize) {}
