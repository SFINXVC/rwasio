#![allow(unsafe_op_in_unsafe_fn)]

use crate::asio::*;
use crate::guid;
use core::ffi::{c_char, c_void};
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

pub type HResult = i32;

pub const S_OK: HResult = 0;
pub const S_FALSE: HResult = 1;
pub const E_NOINTERFACE: HResult = 0x8000_4002u32 as i32;
pub const E_POINTER: HResult = 0x8000_4003u32 as i32;
pub const CLASS_E_NOAGGREGATION: HResult = 0x8004_0110u32 as i32;
pub const CLASS_E_CLASSNOTAVAILABLE: HResult = 0x8004_0111u32 as i32;

pub const IID_IUNKNOWN: Guid = guid!("00000000-0000-0000-C000-000000000046");
pub const IID_ICLASSFACTORY: Guid = guid!("00000001-0000-0000-C000-000000000046");

static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);
static SERVER_LOCKS: AtomicU32 = AtomicU32::new(0);

pub trait AsioClass: Asio {
    const CLSID: Guid;
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const DLL_FILE: &'static str;
    const THREADING_MODEL: &'static str = "Apartment";
    fn new() -> Self;
}

#[repr(C)]
pub struct IClassFactoryVtbl {
    pub base: IUnknownVtbl,
    pub create_instance: unsafe extern "win64" fn(
        this: *mut IClassFactory,
        outer: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> HResult,
    pub lock_server: unsafe extern "win64" fn(this: *mut IClassFactory, lock: Bool) -> HResult,
}

#[repr(C)]
pub struct IClassFactory {
    pub lp_vtbl: *const IClassFactoryVtbl,
}

#[repr(C)]
pub struct AsioObject<T: AsioClass> {
    vtbl: *const IAsioVtbl,
    ref_count: AtomicU32,
    inner: T,
}

impl<T: AsioClass> AsioObject<T> {
    const VTBL: IAsioVtbl = IAsioVtbl {
        base: IUnknownVtbl {
            query_interface: Self::query_interface,
            add_ref: Self::add_ref,
            release: Self::release,
        },
        init: Self::init,
        get_driver_name: Self::get_driver_name,
        get_driver_version: Self::get_driver_version,
        get_error_message: Self::get_error_message,
        start: Self::start,
        stop: Self::stop,
        get_channels: Self::get_channels,
        get_latencies: Self::get_latencies,
        get_buffer_size: Self::get_buffer_size,
        can_sample_rate: Self::can_sample_rate,
        get_sample_rate: Self::get_sample_rate,
        set_sample_rate: Self::set_sample_rate,
        get_clock_sources: Self::get_clock_sources,
        set_clock_source: Self::set_clock_source,
        get_sample_position: Self::get_sample_position,
        get_channel_info: Self::get_channel_info,
        create_buffers: Self::create_buffers,
        dispose_buffers: Self::dispose_buffers,
        control_panel: Self::control_panel,
        future: Self::future,
        output_ready: Self::output_ready,
    };

    pub fn new_raw() -> *mut IAsio {
        OBJECT_COUNT.fetch_add(1, Ordering::Relaxed);

        let boxed = Box::new(AsioObject::<T> {
            vtbl: &Self::VTBL,
            ref_count: AtomicU32::new(1),
            inner: T::new(),
        });

        Box::into_raw(boxed) as *mut IAsio
    }

    unsafe extern "win64" fn query_interface(
        this: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> HResult {
        if riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }

        let iid = *riid;
        if iid == IID_IUNKNOWN || iid == T::CLSID {
            *ppv = this as *mut c_void;
            Self::add_ref(this);
            S_OK
        } else {
            *ppv = ptr::null_mut();
            E_NOINTERFACE
        }
    }

    unsafe extern "win64" fn add_ref(this: *mut IUnknown) -> u32 {
        let obj = &*(this as *const AsioObject<T>);
        obj.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    unsafe extern "win64" fn release(this: *mut IUnknown) -> u32 {
        let obj = &*(this as *const AsioObject<T>);
        let prev = obj.ref_count.fetch_sub(1, Ordering::AcqRel);

        if prev == 1 {
            drop(Box::from_raw(this as *mut AsioObject<T>));
            OBJECT_COUNT.fetch_sub(1, Ordering::Relaxed);
            0
        } else {
            prev - 1
        }
    }

    unsafe extern "win64" fn init(this: *mut IAsio, sys_handle: *mut c_void) -> Bool {
        let obj = &mut *(this as *mut AsioObject<T>);
        obj.inner.init(sys_handle as usize)
    }

    unsafe extern "win64" fn get_driver_name(this: *mut IAsio, name: *mut c_char) {
        let obj = &*(this as *const AsioObject<T>);
        if name.is_null() {
            return;
        }
        let s = obj.inner.get_driver_name().as_bytes();
        let n = s.len().min(31);
        ptr::copy_nonoverlapping(s.as_ptr(), name as *mut u8, n);
        *name.add(n) = 0;
    }

    unsafe extern "win64" fn get_driver_version(this: *mut IAsio) -> i32 {
        let obj = &*(this as *const AsioObject<T>);
        obj.inner.get_driver_version()
    }

    unsafe extern "win64" fn get_error_message(this: *mut IAsio, string: *mut c_char) {
        let obj = &*(this as *const AsioObject<T>);
        if string.is_null() {
            return;
        }
        let s = obj.inner.get_error_message().as_bytes();
        let n = s.len().min(123);
        ptr::copy_nonoverlapping(s.as_ptr(), string as *mut u8, n);
        *string.add(n) = 0;
    }

    unsafe extern "win64" fn start(this: *mut IAsio) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.start() {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn stop(this: *mut IAsio) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.stop() {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_channels(
        this: *mut IAsio,
        num_in: *mut i32,
        num_out: *mut i32,
    ) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.get_channels() {
            Ok((i, o)) => {
                *num_in = i;
                *num_out = o;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_latencies(
        this: *mut IAsio,
        in_lat: *mut i32,
        out_lat: *mut i32,
    ) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.get_latencies() {
            Ok((i, o)) => {
                *in_lat = i;
                *out_lat = o;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_buffer_size(
        this: *mut IAsio,
        min: *mut i32,
        max: *mut i32,
        pref: *mut i32,
        gran: *mut i32,
    ) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.get_buffer_size() {
            Ok((mn, mx, pr, gr)) => {
                *min = mn;
                *max = mx;
                *pref = pr;
                *gran = gr;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn can_sample_rate(this: *mut IAsio, rate: SampleRate) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.can_sample_rate(rate) {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_sample_rate(this: *mut IAsio, rate: *mut SampleRate) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.get_sample_rate() {
            Ok(r) => {
                *rate = r;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn set_sample_rate(this: *mut IAsio, rate: SampleRate) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.set_sample_rate(rate) {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_clock_sources(
        this: *mut IAsio,
        clocks: *mut ClockSource,
        num: *mut i32,
    ) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        if clocks.is_null() || num.is_null() {
            return Error::InvalidParameter;
        }
        let n = *num;
        if n <= 0 {
            return Error::InvalidParameter;
        }
        let slice = core::slice::from_raw_parts_mut(clocks, n as usize);
        match obj.inner.get_clock_sources(slice) {
            Ok(n) => {
                *num = n;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn set_clock_source(this: *mut IAsio, reference: i32) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.set_clock_source(reference) {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_sample_position(
        this: *mut IAsio,
        s_pos: *mut Samples,
        t_stamp: *mut TimeStamp,
    ) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        match obj.inner.get_sample_position() {
            Ok((sp, ts)) => {
                *s_pos = sp;
                *t_stamp = ts;
                Error::Ok
            }
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn get_channel_info(this: *mut IAsio, info: *mut ChannelInfo) -> Error {
        let obj = &*(this as *const AsioObject<T>);
        if info.is_null() {
            return Error::InvalidParameter;
        }
        match obj.inner.get_channel_info(&mut *info) {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn create_buffers(
        this: *mut IAsio,
        buffer_infos: *mut BufferInfo,
        num_channels: i32,
        buffer_size: i32,
        callbacks: *mut Callbacks,
    ) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        if buffer_infos.is_null() || callbacks.is_null() {
            return Error::InvalidParameter;
        }
        if num_channels <= 0 {
            return Error::InvalidParameter;
        }
        let slice = core::slice::from_raw_parts_mut(buffer_infos, num_channels as usize);
        match obj
            .inner
            .create_buffers(slice, buffer_size, &mut *callbacks)
        {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn dispose_buffers(this: *mut IAsio) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.dispose_buffers() {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn control_panel(this: *mut IAsio) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.control_panel() {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn future(this: *mut IAsio, selector: i32, opt: *mut c_void) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.future(selector, opt as usize) {
            Ok(()) => Error::Success,
            Err(e) => Error::from(e),
        }
    }

    unsafe extern "win64" fn output_ready(this: *mut IAsio) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.output_ready() {
            Ok(()) => Error::Ok,
            Err(e) => Error::from(e),
        }
    }
}

#[repr(C)]
pub struct ClassFactory<T: AsioClass> {
    vtbl: *const IClassFactoryVtbl,
    ref_count: AtomicU32,
    _marker: PhantomData<T>,
}

impl<T: AsioClass> ClassFactory<T> {
    const VTBL: IClassFactoryVtbl = IClassFactoryVtbl {
        base: IUnknownVtbl {
            query_interface: Self::query_interface,
            add_ref: Self::add_ref,
            release: Self::release,
        },
        create_instance: Self::create_instance,
        lock_server: Self::lock_server,
    };

    pub fn new_raw() -> *mut IClassFactory {
        let boxed = Box::new(ClassFactory::<T> {
            vtbl: &Self::VTBL,
            ref_count: AtomicU32::new(1),
            _marker: PhantomData,
        });

        Box::into_raw(boxed) as *mut IClassFactory
    }

    unsafe extern "win64" fn query_interface(
        this: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> HResult {
        if riid.is_null() || ppv.is_null() {
            return E_POINTER;
        }

        let iid = *riid;
        tracing::trace!(
            "ClassFactory::query_interface iid={:08x}-{:04x}-{:04x}",
            iid.data1,
            iid.data2,
            iid.data3
        );

        if iid == IID_IUNKNOWN || iid == IID_ICLASSFACTORY {
            tracing::trace!("ClassFactory::query_interface matched -> S_OK");
            *ppv = this as *mut c_void;
            Self::add_ref(this);
            S_OK
        } else {
            tracing::trace!("ClassFactory::query_interface no match -> E_NOINTERFACE");
            *ppv = ptr::null_mut();
            E_NOINTERFACE
        }
    }

    unsafe extern "win64" fn add_ref(this: *mut IUnknown) -> u32 {
        let factory = &*(this as *const ClassFactory<T>);
        factory.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    unsafe extern "win64" fn release(this: *mut IUnknown) -> u32 {
        let factory = &*(this as *const ClassFactory<T>);
        let prev = factory.ref_count.fetch_sub(1, Ordering::AcqRel);

        if prev == 1 {
            drop(Box::from_raw(this as *mut ClassFactory<T>));
            0
        } else {
            prev - 1
        }
    }

    unsafe extern "win64" fn create_instance(
        _this: *mut IClassFactory,
        outer: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> HResult {
        tracing::trace!("ClassFactory::create_instance called");
        if riid.is_null() {
            tracing::trace!("create_instance: riid null -> E_POINTER");
            return E_POINTER;
        }
        if ppv.is_null() {
            tracing::trace!("create_instance: ppv null -> E_POINTER");
            return E_POINTER;
        }

        let iid = *riid;
        tracing::trace!(
            "create_instance riid={:08x}-{:04x}-{:04x}",
            iid.data1,
            iid.data2,
            iid.data3
        );

        *ppv = ptr::null_mut();

        if !outer.is_null() {
            tracing::trace!("create_instance: outer non-null -> CLASS_E_NOAGGREGATION");
            return CLASS_E_NOAGGREGATION;
        }

        let object = AsioObject::<T>::new_raw();
        let unknown = object as *mut IUnknown;
        let vtbl = (*unknown).lp_vtbl;
        let hr = ((*vtbl).query_interface)(unknown, riid, ppv);
        tracing::trace!("create_instance: query_interface hr={hr:#010x}");
        ((*vtbl).release)(unknown);

        hr
    }

    unsafe extern "win64" fn lock_server(_this: *mut IClassFactory, lock: Bool) -> HResult {
        if lock.as_bool() {
            SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
        } else {
            SERVER_LOCKS.fetch_sub(1, Ordering::Relaxed);
        }

        S_OK
    }
}

/// # Safety
///
/// `rclsid` and `riid` must be valid non-null pointers to `GUID`. `ppv` must be
/// either null or a valid pointer to a `*mut c_void` that can be written to.
pub unsafe fn dll_get_class_object<T: AsioClass>(
    rclsid: Refiid,
    riid: Refiid,
    ppv: *mut *mut c_void,
) -> HResult {
    crate::init_logging();
    tracing::trace!("DllGetClassObject called ppv={:?} rclsid={:?}", ppv, rclsid);

    tracing::trace!("checking ppv/rclsid/riid null");
    if ppv.is_null() || rclsid.is_null() || riid.is_null() {
        tracing::trace!("ppv/rclsid/riid null -> E_POINTER");
        return E_POINTER;
    }

    tracing::trace!("zeroing ppv");
    *ppv = ptr::null_mut();

    tracing::trace!("reading rclsid");
    let clsid = *rclsid;
    tracing::trace!(
        "rclsid={:08x}-{:04x}-{:04x}",
        clsid.data1,
        clsid.data2,
        clsid.data3
    );

    if clsid != T::CLSID {
        tracing::trace!("clsid mismatch -> CLASS_E_CLASSNOTAVAILABLE");
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    tracing::trace!("clsid matched, creating factory");
    let factory = ClassFactory::<T>::new_raw();
    let unknown = factory as *mut IUnknown;
    let vtbl = (*unknown).lp_vtbl;
    let hr = ((*vtbl).query_interface)(unknown, riid, ppv);
    tracing::trace!("DllGetClassObject: query_interface hr={hr:#010x}");
    ((*vtbl).release)(unknown);

    hr
}

pub fn dll_can_unload_now() -> HResult {
    if crate::MODULE_PINNED.load(Ordering::SeqCst) {
        return S_FALSE;
    }

    if OBJECT_COUNT.load(Ordering::Relaxed) == 0 && SERVER_LOCKS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

pub const SELFREG_E_CLASS: HResult = 0x8004_0201u32 as i32;

const HKEY_CLASSES_ROOT: usize = 0xFFFF_FFFF_8000_0000usize;
const HKEY_LOCAL_MACHINE: usize = 0xFFFF_FFFF_8000_0002usize;
const REG_SZ: u32 = 1;
const ERROR_SUCCESS: i32 = 0;
unsafe extern "win64" {
    fn RegOpenKeyW(hkey: usize, lpsubkey: *const u16, phkresult: *mut usize) -> i32;
    fn RegCreateKeyW(hkey: usize, lpsubkey: *const u16, phkresult: *mut usize) -> i32;
    fn RegSetValueExW(
        hkey: usize,
        lpvaluename: *const u16,
        reserved: u32,
        dwtype: u32,
        lpdata: *const u8,
        cbdata: u32,
    ) -> i32;
    fn RegQueryValueExW(
        hkey: usize,
        lpvaluename: *const u16,
        reserved: *mut u32,
        lptype: *mut u32,
        lpdata: *mut u8,
        lpcbdata: *mut u32,
    ) -> i32;
    fn RegCloseKey(hkey: usize) -> i32;
    fn RegDeleteTreeW(hkey: usize, lpsubkey: *const u16) -> i32;
}

fn guid_to_str(g: &Guid) -> [u8; 40] {
    const HEX: &[u8; 16] = b"0123456789ABCDEF";
    let mut buf = [0u8; 40];
    let mut i = 0;
    macro_rules! pc {
        ($c:expr) => {
            buf[i] = $c;
            i += 1;
            let _ = i;
        };
    }
    macro_rules! ph {
        ($v:expr) => {
            let v = $v as u8;
            pc!(HEX[((v >> 4) & 0xF) as usize]);
            pc!(HEX[(v & 0xF) as usize]);
        };
    }
    pc!(b'{');
    ph!(g.data1 >> 24);
    ph!(g.data1 >> 16);
    ph!(g.data1 >> 8);
    ph!(g.data1);
    pc!(b'-');
    ph!(g.data2 >> 8);
    ph!(g.data2);
    pc!(b'-');
    ph!(g.data3 >> 8);
    ph!(g.data3);
    pc!(b'-');
    ph!(g.data4[0]);
    ph!(g.data4[1]);
    pc!(b'-');
    ph!(g.data4[2]);
    ph!(g.data4[3]);
    ph!(g.data4[4]);
    ph!(g.data4[5]);
    ph!(g.data4[6]);
    ph!(g.data4[7]);
    pc!(b'}');
    buf
}

fn bytes_cat(buf: &mut [u8], start: usize, src: &[u8]) -> usize {
    let mut n = start;
    for &b in src {
        if b == 0 || n + 1 >= buf.len() {
            break;
        }
        buf[n] = b;
        n += 1;
    }
    buf[n] = 0;
    n
}

fn words_cat(buf: &mut [u16], start: usize, src: &[u8]) -> usize {
    let mut n = start;
    for &b in src {
        if b == 0 || n + 1 >= buf.len() {
            break;
        }
        buf[n] = b as u16;
        n += 1;
    }
    buf[n] = 0;
    n
}

fn ascii_lower_wide(buf: &mut [u16]) {
    for b in buf.iter_mut() {
        if *b == 0 {
            break;
        }
        if *b >= b'A' as u16 && *b <= b'Z' as u16 {
            *b += 32;
        }
    }
}

fn cstr_eq_wide(a: &[u16], b: &[u8]) -> bool {
    let la = a.iter().position(|&x| x == 0).unwrap_or(a.len());
    let lb = b.iter().position(|&x| x == 0).unwrap_or(b.len());
    la == lb
        && a[..la]
            .iter()
            .zip(b[..lb].iter())
            .all(|(&aw, &bw)| aw == bw as u16)
}

unsafe fn reg_exists(root: usize, path: &[u8]) -> bool {
    let mut wpath = [0u16; 256];
    words_cat(&mut wpath, 0, path);
    let mut hkey = 0usize;
    let ok = RegOpenKeyW(root, wpath.as_ptr(), &mut hkey) == ERROR_SUCCESS;
    if ok {
        RegCloseKey(hkey);
    }
    ok
}

unsafe fn reg_get_sz(root: usize, path: &[u8], valname: *const u8, buf: &mut [u16]) -> bool {
    let mut wpath = [0u16; 256];
    words_cat(&mut wpath, 0, path);
    let mut hkey = 0usize;
    if RegOpenKeyW(root, wpath.as_ptr(), &mut hkey) != ERROR_SUCCESS {
        return false;
    }
    let mut wname = [0u16; 64];
    let wname_ptr: *const u16 = if valname.is_null() {
        ptr::null()
    } else {
        let mut i = 0usize;
        while i + 1 < 64 {
            let b = *valname.add(i);
            if b == 0 {
                break;
            }
            wname[i] = b as u16;
            i += 1;
        }
        wname.as_ptr()
    };
    let mut typ = 0u32;
    let mut sz = (buf.len() * 2) as u32;
    let ok = RegQueryValueExW(
        hkey,
        wname_ptr,
        ptr::null_mut(),
        &mut typ,
        buf.as_mut_ptr() as *mut u8,
        &mut sz,
    ) == ERROR_SUCCESS;
    RegCloseKey(hkey);
    ok
}

unsafe fn reg_create_path(root: usize, path: &[u8]) -> Option<usize> {
    let mut wpath = [0u16; 256];
    words_cat(&mut wpath, 0, path);
    let mut hkey = 0usize;
    if RegCreateKeyW(root, wpath.as_ptr(), &mut hkey) == ERROR_SUCCESS {
        Some(hkey)
    } else {
        None
    }
}

unsafe fn reg_set_sz(hkey: usize, valname: *const u8, value: &[u8]) {
    let mut vname_wide = [0u16; 64];
    let vname_ptr: *const u16 = if valname.is_null() {
        ptr::null()
    } else {
        let mut i = 0usize;
        while i + 1 < 64 {
            let b = *valname.add(i);
            if b == 0 {
                break;
            }
            vname_wide[i] = b as u16;
            i += 1;
        }
        vname_wide.as_ptr()
    };
    let mut data_wide = [0u16; 512];
    let mut nchars = 0usize;
    for (i, &b) in value.iter().enumerate() {
        if b == 0 || i + 1 >= 512 {
            break;
        }
        data_wide[i] = b as u16;
        nchars = i + 1;
    }
    let cbdata = ((nchars + 1) * 2) as u32;
    RegSetValueExW(
        hkey,
        vname_ptr,
        0,
        REG_SZ,
        data_wide.as_ptr() as *const u8,
        cbdata,
    );
}

/// # Safety
///
/// Called by COM runtime / regsvr32. Writes ASIO driver entries to the Windows registry.
/// Must run inside a Wine process where Win32 APIs are available.
pub unsafe fn dll_register_server<T: AsioClass>() -> HResult {
    crate::init_logging();
    let dll_path = T::DLL_FILE.as_bytes();
    let clsid = guid_to_str(&T::CLSID);

    let mut clsid_path = [0u8; 64];
    let cn = bytes_cat(&mut clsid_path, 0, b"CLSID\\");
    let cn = bytes_cat(&mut clsid_path, cn, &clsid[..38]);

    let mut new_entry = true;
    if reg_exists(HKEY_CLASSES_ROOT, &clsid_path) {
        let mut ip_path = [0u8; 80];
        let m = bytes_cat(&mut ip_path, 0, &clsid_path[..cn]);
        bytes_cat(&mut ip_path, m, b"\\InprocServer32");
        let mut existing = [0u16; 180];
        if reg_get_sz(HKEY_CLASSES_ROOT, &ip_path, ptr::null(), &mut existing) {
            ascii_lower_wide(&mut existing);
            if cstr_eq_wide(&existing, dll_path) {
                new_entry = false;
            } else {
                let mut wdel = [0u16; 64];
                words_cat(&mut wdel, 0, &clsid_path);
                RegDeleteTreeW(HKEY_CLASSES_ROOT, wdel.as_ptr());
            }
        }
    }

    if new_entry {
        let hk = match reg_create_path(HKEY_CLASSES_ROOT, &clsid_path) {
            Some(h) => h,
            None => return SELFREG_E_CLASS,
        };
        reg_set_sz(hk, ptr::null(), T::DESCRIPTION.as_bytes());
        RegCloseKey(hk);

        let mut ip_path = [0u8; 80];
        let m = bytes_cat(&mut ip_path, 0, &clsid_path[..cn]);
        bytes_cat(&mut ip_path, m, b"\\InprocServer32");
        let hk = match reg_create_path(HKEY_CLASSES_ROOT, &ip_path) {
            Some(h) => h,
            None => return SELFREG_E_CLASS,
        };
        reg_set_sz(hk, ptr::null(), dll_path);
        reg_set_sz(
            hk,
            c"ThreadingModel".as_ptr() as *const u8,
            T::THREADING_MODEL.as_bytes(),
        );
        RegCloseKey(hk);
    }

    let mut asio_path = [0u8; 32];
    bytes_cat(&mut asio_path, 0, b"SOFTWARE\\ASIO");

    if reg_exists(HKEY_LOCAL_MACHINE, &asio_path) {
        let mut driver_path = [0u8; 256];
        let p = bytes_cat(&mut driver_path, 0, b"SOFTWARE\\ASIO\\");
        bytes_cat(&mut driver_path, p, T::NAME.as_bytes());
        if reg_exists(HKEY_LOCAL_MACHINE, &driver_path) {
            let mut wdel = [0u16; 256];
            words_cat(&mut wdel, 0, &driver_path);
            RegDeleteTreeW(HKEY_LOCAL_MACHINE, wdel.as_ptr());
        }
    } else {
        let hk = match reg_create_path(HKEY_LOCAL_MACHINE, &asio_path) {
            Some(h) => h,
            None => return SELFREG_E_CLASS,
        };
        RegCloseKey(hk);
    }

    let mut driver_path = [0u8; 256];
    let p = bytes_cat(&mut driver_path, 0, b"SOFTWARE\\ASIO\\");
    bytes_cat(&mut driver_path, p, T::NAME.as_bytes());
    let hk = match reg_create_path(HKEY_LOCAL_MACHINE, &driver_path) {
        Some(h) => h,
        None => return SELFREG_E_CLASS,
    };
    reg_set_sz(hk, c"CLSID".as_ptr() as *const u8, &clsid[..39]);
    reg_set_sz(
        hk,
        c"Description".as_ptr() as *const u8,
        T::DESCRIPTION.as_bytes(),
    );
    RegCloseKey(hk);

    S_OK
}

/// # Safety
///
/// Called by regsvr32 /u. Removes ASIO driver registry entries written by `DllRegisterServer`.
pub unsafe fn dll_unregister_server<T: AsioClass>() -> HResult {
    crate::init_logging();
    let clsid = guid_to_str(&T::CLSID);

    let mut clsid_path = [0u8; 64];
    let cn = bytes_cat(&mut clsid_path, 0, b"CLSID\\");
    bytes_cat(&mut clsid_path, cn, &clsid[..38]);
    if reg_exists(HKEY_CLASSES_ROOT, &clsid_path) {
        let mut wdel = [0u16; 64];
        words_cat(&mut wdel, 0, &clsid_path);
        RegDeleteTreeW(HKEY_CLASSES_ROOT, wdel.as_ptr());
    }

    let mut asio_path = [0u8; 32];
    bytes_cat(&mut asio_path, 0, b"SOFTWARE\\ASIO");
    if reg_exists(HKEY_LOCAL_MACHINE, &asio_path) {
        let mut driver_path = [0u8; 256];
        let p = bytes_cat(&mut driver_path, 0, b"SOFTWARE\\ASIO\\");
        bytes_cat(&mut driver_path, p, T::NAME.as_bytes());
        if reg_exists(HKEY_LOCAL_MACHINE, &driver_path) {
            let mut wdel = [0u16; 256];
            words_cat(&mut wdel, 0, &driver_path);
            RegDeleteTreeW(HKEY_LOCAL_MACHINE, wdel.as_ptr());
        }
    }

    S_OK
}

#[macro_export]
macro_rules! export_asio_driver {
    ($t:ty) => {
        /// # Safety
        ///
        /// Called by COM runtime. `rclsid` and `riid` must be valid non-null `GUID`
        /// pointers. `ppv` must be null or a valid writable `*mut c_void` pointer.
        #[unsafe(no_mangle)]
        pub unsafe extern "win64" fn DllGetClassObject(
            rclsid: $crate::asio::Refiid,
            riid: $crate::asio::Refiid,
            ppv: *mut *mut core::ffi::c_void,
        ) -> i32 {
            unsafe { $crate::com::dll_get_class_object::<$t>(rclsid, riid, ppv) }
        }

        /// # Safety
        ///
        /// Called by COM runtime to check if the DLL can be unloaded.
        #[unsafe(no_mangle)]
        pub unsafe extern "win64" fn DllCanUnloadNow() -> i32 {
            $crate::com::dll_can_unload_now()
        }

        /// # Safety
        ///
        /// Called by regsvr32 to register the ASIO driver in the Wine registry.
        #[unsafe(no_mangle)]
        pub unsafe extern "win64" fn DllRegisterServer() -> i32 {
            unsafe { $crate::com::dll_register_server::<$t>() }
        }

        /// # Safety
        ///
        /// Called by regsvr32 /u to remove ASIO driver registry entries.
        #[unsafe(no_mangle)]
        pub unsafe extern "win64" fn DllUnregisterServer() -> i32 {
            unsafe { $crate::com::dll_unregister_server::<$t>() }
        }
    };
}
