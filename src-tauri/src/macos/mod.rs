//! Native system signals. macOS: memory-pressure events from a dispatch
//! source (no polling), thermal state and Low Power Mode from
//! NSProcessInfo, power source from IOKit, child processes from libproc.
//! Other platforms get conservative stubs.

use serde::Serialize;

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Pressure {
    Normal,
    Warning,
    Critical,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "snake_case")]
pub enum Thermal {
    Nominal,
    Fair,
    Serious,
    Critical,
}

#[derive(Serialize, Clone, Copy, Debug, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum PowerSource {
    Ac,
    Battery,
    Unknown,
}

#[derive(Serialize, Clone, Debug)]
pub struct Hardware {
    pub chip: String,
    pub total_mem_gb: f64,
    pub cores: usize,
    pub apple_silicon: bool,
    pub os: String,
}

pub fn hardware() -> Hardware {
    let total = total_memory_bytes() as f64 / 1_073_741_824.0;
    Hardware {
        chip: chip_name(),
        total_mem_gb: (total * 10.0).round() / 10.0,
        cores: std::thread::available_parallelism().map(|n| n.get()).unwrap_or(1),
        apple_silicon: cfg!(all(target_os = "macos", target_arch = "aarch64")),
        os: std::env::consts::OS.to_string(),
    }
}

/// Free disk space (bytes) on the volume holding `path`.
pub fn free_disk_bytes(path: &std::path::Path) -> Option<u64> {
    #[cfg(unix)]
    {
        use std::ffi::CString;
        let mut p = path.to_path_buf();
        while !p.exists() {
            p = p.parent()?.to_path_buf();
        }
        let c = CString::new(p.to_string_lossy().as_bytes()).ok()?;
        let mut s: libc::statvfs = unsafe { std::mem::zeroed() };
        if unsafe { libc::statvfs(c.as_ptr(), &mut s) } != 0 {
            return None;
        }
        Some(s.f_bavail as u64 * s.f_frsize as u64)
    }
    #[cfg(not(unix))]
    {
        let _ = path;
        None
    }
}

#[cfg(target_os = "macos")]
mod imp {
    use super::*;
    use std::ffi::{c_char, c_void, CStr};

    fn sysctl_bytes(name: &str) -> Option<Vec<u8>> {
        let cname = std::ffi::CString::new(name).ok()?;
        let mut len: libc::size_t = 0;
        unsafe {
            if libc::sysctlbyname(cname.as_ptr(), std::ptr::null_mut(), &mut len, std::ptr::null_mut(), 0) != 0 {
                return None;
            }
            let mut buf = vec![0u8; len];
            if libc::sysctlbyname(cname.as_ptr(), buf.as_mut_ptr() as *mut c_void, &mut len, std::ptr::null_mut(), 0) != 0 {
                return None;
            }
            buf.truncate(len);
            Some(buf)
        }
    }

    pub fn chip_name() -> String {
        sysctl_bytes("machdep.cpu.brand_string")
            .and_then(|b| CStr::from_bytes_until_nul(&b).ok().map(|c| c.to_string_lossy().into_owned()))
            .unwrap_or_else(|| "Unknown chip".into())
    }

    pub fn total_memory_bytes() -> u64 {
        sysctl_bytes("hw.memsize").and_then(|b| b.try_into().ok()).map(u64::from_ne_bytes).unwrap_or(0)
    }

    /// Memory macOS considers available (free + reclaimable), from the
    /// kernel's own "free percentage" (`kern.memorystatus_level`, the value
    /// `memory_pressure` prints). `sysinfo`'s figure reads ~0 on macOS.
    pub fn available_memory_bytes() -> u64 {
        let pct = sysctl_bytes("kern.memorystatus_level")
            .and_then(|b| b.get(..4).map(|s| i32::from_ne_bytes(s.try_into().unwrap())))
            .filter(|p| (1..=100).contains(p))
            .unwrap_or(0);
        total_memory_bytes() / 100 * pct as u64
    }

    pub fn pressure_now() -> Pressure {
        let v = sysctl_bytes("kern.memorystatus_vm_pressure_level")
            .and_then(|b| b.get(..4).map(|s| i32::from_ne_bytes(s.try_into().unwrap())))
            .unwrap_or(1);
        match v {
            4 => Pressure::Critical,
            2 => Pressure::Warning,
            _ => Pressure::Normal,
        }
    }

    pub fn thermal_now() -> Thermal {
        use objc2_foundation::NSProcessInfo;
        let s = NSProcessInfo::processInfo().thermalState();
        match s.0 {
            0 => Thermal::Nominal,
            1 => Thermal::Fair,
            2 => Thermal::Serious,
            _ => Thermal::Critical,
        }
    }

    pub fn low_power_mode() -> bool {
        use objc2_foundation::NSProcessInfo;
        NSProcessInfo::processInfo().isLowPowerModeEnabled()
    }

    type CFTypeRef = *const c_void;
    #[link(name = "IOKit", kind = "framework")]
    extern "C" {
        fn IOPSCopyPowerSourcesInfo() -> CFTypeRef;
        fn IOPSGetProvidingPowerSourceType(snapshot: CFTypeRef) -> CFTypeRef;
    }
    #[link(name = "CoreFoundation", kind = "framework")]
    extern "C" {
        fn CFStringGetCString(s: CFTypeRef, buf: *mut c_char, size: isize, encoding: u32) -> bool;
        fn CFRelease(cf: CFTypeRef);
    }

    pub fn power_source() -> PowerSource {
        unsafe {
            let info = IOPSCopyPowerSourcesInfo();
            if info.is_null() {
                return PowerSource::Unknown;
            }
            let s = IOPSGetProvidingPowerSourceType(info); // "Get" rule: not retained
            let mut buf = [0 as c_char; 64];
            let ok = !s.is_null() && CFStringGetCString(s, buf.as_mut_ptr(), 64, 0x0800_0100);
            CFRelease(info);
            if !ok {
                return PowerSource::Unknown;
            }
            match CStr::from_ptr(buf.as_ptr()).to_str().unwrap_or("") {
                "Battery Power" => PowerSource::Battery,
                "AC Power" | "UPS Power" => PowerSource::Ac,
                _ => PowerSource::Unknown,
            }
        }
    }

    // ---- memory-pressure dispatch source ----
    extern "C" {
        static _dispatch_source_type_memorypressure: c_void;
        fn dispatch_source_create(t: *const c_void, handle: usize, mask: usize, queue: *mut c_void) -> *mut c_void;
        fn dispatch_get_global_queue(identifier: isize, flags: usize) -> *mut c_void;
        fn dispatch_set_context(obj: *mut c_void, ctx: *mut c_void);
        fn dispatch_source_set_event_handler_f(source: *mut c_void, handler: extern "C" fn(*mut c_void));
        fn dispatch_source_get_data(source: *mut c_void) -> usize;
        fn dispatch_resume(obj: *mut c_void);
    }
    const WARN: usize = 0x2;
    const CRITICAL: usize = 0x4;
    const NORMAL: usize = 0x1;
    const QOS_CLASS_UTILITY: isize = 0x11;

    struct Ctx {
        source: *mut c_void,
        cb: Box<dyn Fn(Pressure) + Send + Sync>,
    }

    extern "C" fn on_event(ctx: *mut c_void) {
        // SAFETY: ctx is the leaked Box<Ctx> registered below; it lives for the process.
        let c = unsafe { &*(ctx as *const Ctx) };
        let data = unsafe { dispatch_source_get_data(c.source) };
        let level = if data & CRITICAL != 0 {
            Pressure::Critical
        } else if data & WARN != 0 {
            Pressure::Warning
        } else {
            Pressure::Normal
        };
        (c.cb)(level);
    }

    /// Registers `cb` for kernel memory-pressure transitions. Costs nothing
    /// until the kernel signals; lives for the life of the process.
    pub fn watch_pressure(cb: impl Fn(Pressure) + Send + Sync + 'static) {
        unsafe {
            let q = dispatch_get_global_queue(QOS_CLASS_UTILITY, 0);
            let src = dispatch_source_create(&_dispatch_source_type_memorypressure as *const c_void, 0, NORMAL | WARN | CRITICAL, q);
            if src.is_null() {
                return;
            }
            let ctx = Box::into_raw(Box::new(Ctx { source: src, cb: Box::new(cb) }));
            dispatch_set_context(src, ctx as *mut c_void);
            dispatch_source_set_event_handler_f(src, on_event);
            dispatch_resume(src);
        }
    }

    // ---- process trees ----
    extern "C" {
        fn proc_listchildpids(ppid: libc::pid_t, buffer: *mut c_void, buffersize: libc::c_int) -> libc::c_int;
    }

    pub fn children(pid: u32) -> Vec<u32> {
        let mut buf = vec![0 as libc::pid_t; 256];
        let n = unsafe {
            proc_listchildpids(
                pid as libc::pid_t,
                buf.as_mut_ptr() as *mut c_void,
                (buf.len() * std::mem::size_of::<libc::pid_t>()) as libc::c_int,
            )
        };
        if n <= 0 {
            return vec![];
        }
        buf.truncate(n as usize);
        buf.into_iter().filter(|p| *p > 0).map(|p| p as u32).collect()
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use super::*;
    pub fn chip_name() -> String {
        std::env::consts::ARCH.to_string()
    }
    pub fn total_memory_bytes() -> u64 {
        let mut s = sysinfo::System::new();
        s.refresh_memory();
        s.total_memory()
    }
    pub fn available_memory_bytes() -> u64 {
        let mut s = sysinfo::System::new();
        s.refresh_memory();
        s.available_memory()
    }
    pub fn pressure_now() -> Pressure {
        Pressure::Normal
    }
    pub fn thermal_now() -> Thermal {
        Thermal::Nominal
    }
    pub fn low_power_mode() -> bool {
        false
    }
    pub fn power_source() -> PowerSource {
        PowerSource::Unknown
    }
    pub fn watch_pressure(_cb: impl Fn(Pressure) + Send + Sync + 'static) {}
    pub fn children(_pid: u32) -> Vec<u32> {
        vec![]
    }
}

pub use imp::{
    available_memory_bytes, children, chip_name, low_power_mode, power_source, pressure_now, thermal_now, total_memory_bytes,
    watch_pressure,
};

/// All descendants of `pid` (inclusive), breadth-first, capped for safety.
pub fn process_tree(pid: u32) -> Vec<u32> {
    let mut out = vec![pid];
    let mut i = 0;
    while i < out.len() && out.len() < 512 {
        let kids = children(out[i]);
        out.extend(kids);
        i += 1;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_hardware_and_signals() {
        let h = hardware();
        assert!(h.total_mem_gb > 0.5);
        let avail = available_memory_bytes();
        assert!(avail > 100 * 1024 * 1024, "available memory should be real, got {avail}");
        assert!(avail <= total_memory_bytes());
        assert!(!h.chip.is_empty());
        let _ = pressure_now();
        let _ = thermal_now();
        let _ = power_source();
        let _ = low_power_mode();
        assert!(free_disk_bytes(std::path::Path::new("/")).unwrap_or(1) > 0);
    }

    #[test]
    #[cfg(target_os = "macos")]
    fn finds_child_processes() {
        let mut child = std::process::Command::new("sleep").arg("5").spawn().unwrap();
        let me = std::process::id();
        let tree = process_tree(me);
        assert!(tree.contains(&child.id()), "{tree:?}");
        child.kill().ok();
        child.wait().ok();
    }
}
