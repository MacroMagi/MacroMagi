//! Graphics cards: their names, for choosing where AI models run, and
//! loading the libraries a graphics card runtime needs.
//!
//! On Windows the cards are listed through DXGI, in the order DirectML
//! numbers them. DXGI is loaded at run time, so nothing is linked against it.

/// A graphics card.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Gpu {
    /// Its number in the system's list (what DirectML calls the device id).
    pub index: u32,
    pub name: String,
    /// Dedicated video memory, in megabytes.
    pub memory_mb: u64,
}

/// The hardware graphics cards, in the system's order. Software renderers are left out.
#[cfg(windows)]
pub fn gpus() -> Vec<Gpu> {
    use std::ffi::c_void;
    use std::ptr;

    use windows_sys::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
    use windows_sys::core::GUID;

    #[repr(C)]
    struct AdapterDesc1 {
        description: [u16; 128],
        vendor_id: u32,
        device_id: u32,
        sub_sys_id: u32,
        revision: u32,
        dedicated_video_memory: usize,
        dedicated_system_memory: usize,
        shared_system_memory: usize,
        luid_low: u32,
        luid_high: i32,
        flags: u32,
    }
    const SOFTWARE: u32 = 2;
    // IID_IDXGIFactory1
    const FACTORY1: GUID = GUID::from_u128(0x770a_ae78_f26f_4dba_a829_253c_83d1_b387);
    type CreateFactory = unsafe extern "system" fn(*const GUID, *mut *mut c_void) -> i32;
    type EnumAdapters1 = unsafe extern "system" fn(*mut c_void, u32, *mut *mut c_void) -> i32;
    type GetDesc1 = unsafe extern "system" fn(*mut c_void, *mut AdapterDesc1) -> i32;
    type Release = unsafe extern "system" fn(*mut c_void) -> u32;

    /// The `index`-th entry of a COM object's table of functions.
    unsafe fn method(object: *mut c_void, index: usize) -> *const c_void {
        // SAFETY: (caller) `object` is a live COM object with at least `index + 1` methods.
        unsafe { *(*(object as *const *const *const c_void)).add(index) }
    }

    let mut list = Vec::new();
    let name: Vec<u16> = "dxgi.dll\0".encode_utf16().collect();
    // SAFETY: DXGI is used exactly as documented: create a factory, enumerate
    // adapters (IDXGIFactory1::EnumAdapters1 is method 12, IDXGIAdapter1::GetDesc1
    // method 10, Release method 2), release everything.
    unsafe {
        let module = LoadLibraryW(name.as_ptr());
        if module.is_null() {
            return list;
        }
        let Some(create) = GetProcAddress(module, c"CreateDXGIFactory1".as_ptr().cast()) else { return list };
        let create: CreateFactory = std::mem::transmute(create);
        let mut factory: *mut c_void = ptr::null_mut();
        if create(&FACTORY1, &mut factory) < 0 || factory.is_null() {
            return list;
        }
        let enumerate: EnumAdapters1 = std::mem::transmute(method(factory, 12));
        for index in 0.. {
            let mut adapter: *mut c_void = ptr::null_mut();
            if enumerate(factory, index, &mut adapter) < 0 || adapter.is_null() {
                break;
            }
            let describe: GetDesc1 = std::mem::transmute(method(adapter, 10));
            let mut desc: AdapterDesc1 = std::mem::zeroed();
            if describe(adapter, &mut desc) >= 0 && desc.flags & SOFTWARE == 0 {
                let end = desc.description.iter().position(|c| *c == 0).unwrap_or(desc.description.len());
                list.push(Gpu {
                    index,
                    name: String::from_utf16_lossy(&desc.description[..end]).trim().to_owned(),
                    memory_mb: desc.dedicated_video_memory as u64 / (1024 * 1024),
                });
            }
            let release: Release = std::mem::transmute(method(adapter, 2));
            release(adapter);
        }
        let release: Release = std::mem::transmute(method(factory, 2));
        release(factory);
    }
    list
}

#[cfg(not(windows))]
pub fn gpus() -> Vec<Gpu> {
    Vec::new()
}

/// Loads a library ahead of time, so a library loaded later that asks for it
/// by name gets this copy rather than an older one the system has (DirectML).
#[cfg(windows)]
pub fn preload(path: &std::path::Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    use windows_sys::Win32::System::LibraryLoader::LoadLibraryW;
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: a plain call with a NUL-terminated path; the library stays loaded for the life of the process.
    !unsafe { LoadLibraryW(wide.as_ptr()) }.is_null()
}

#[cfg(not(windows))]
pub fn preload(_path: &std::path::Path) -> bool {
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn graphics_cards_can_be_listed() {
        // Whatever the machine has – possibly none – listing must not fail.
        for gpu in gpus() {
            println!("graphics card {}: {} ({} MB)", gpu.index, gpu.name, gpu.memory_mb);
            assert!(!gpu.name.is_empty());
        }
    }
}
