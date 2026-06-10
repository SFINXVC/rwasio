#![allow(unsafe_op_in_unsafe_fn)]

use crate::asio::*;
use crate::guid;
use core::ffi::{c_char, c_void};
use core::marker::PhantomData;
use core::ptr;
use core::sync::atomic::{AtomicU32, Ordering};

pub type Hresult = i32;

pub const S_OK: Hresult = 0;
pub const S_FALSE: Hresult = 1;
pub const E_NOINTERFACE: Hresult = 0x8000_4002u32 as i32;
pub const E_POINTER: Hresult = 0x8000_4003u32 as i32;
pub const CLASS_E_NOAGGREGATION: Hresult = 0x8004_0110u32 as i32;
pub const CLASS_E_CLASSNOTAVAILABLE: Hresult = 0x8004_0111u32 as i32;

pub const IID_IUNKNOWN: Guid = guid!("00000000-0000-0000-C000-000000000046");
pub const IID_ICLASSFACTORY: Guid = guid!("00000001-0000-0000-C000-000000000046");

static OBJECT_COUNT: AtomicU32 = AtomicU32::new(0);
static SERVER_LOCKS: AtomicU32 = AtomicU32::new(0);

pub trait AsioClass: Asio {
    const CLSID: Guid;
    const NAME: &'static str;
    const DESCRIPTION: &'static str;
    const THREADING_MODEL: &'static str = "Apartment";
    fn new() -> Self;
}

#[repr(C)]
pub struct IClassFactoryVtbl {
    pub base: IUnknownVtbl,
    pub create_instance: unsafe extern "system" fn(
        this: *mut IClassFactory,
        outer: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> Hresult,
    pub lock_server: unsafe extern "system" fn(this: *mut IClassFactory, lock: Bool) -> Hresult,
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

macro_rules! asio_shims {
    (
        $( pass fn $name:ident ( $( $arg:ident : $ty:ty ),* ) -> $ret:ty ; )*
        $( result fn $rname:ident ( $( $rarg:ident : $rty:ty ),* ) ; )*
    ) => {
        $(
            unsafe extern "system" fn $name(this: *mut IAsio $(, $arg: $ty)*) -> $ret {
                let obj = &mut *(this as *mut AsioObject<T>);
                obj.inner.$name($($arg),*)
            }
        )*
        $(
            unsafe extern "system" fn $rname(this: *mut IAsio $(, $rarg: $rty)*) -> Error {
                let obj = &mut *(this as *mut AsioObject<T>);
                match obj.inner.$rname($($rarg),*) {
                    Ok(()) => Error::Ok,
                    Err(e) => Error::from(e),
                }
            }
        )*
    };
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

    unsafe extern "system" fn query_interface(
        this: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> Hresult {
        if ppv.is_null() {
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

    unsafe extern "system" fn add_ref(this: *mut IUnknown) -> u32 {
        let obj = &*(this as *const AsioObject<T>);
        obj.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    unsafe extern "system" fn release(this: *mut IUnknown) -> u32 {
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

    unsafe extern "system" fn future(this: *mut IAsio, selector: i32, opt: *mut c_void) -> Error {
        let obj = &mut *(this as *mut AsioObject<T>);
        match obj.inner.future(selector, opt) {
            Ok(()) => Error::Success,
            Err(e) => Error::from(e),
        }
    }

    asio_shims! {
        pass fn init(sys_handle: *mut c_void) -> Bool;
        pass fn get_driver_name(name: *mut c_char) -> ();
        pass fn get_driver_version() -> i32;
        pass fn get_error_message(string: *mut c_char) -> ();
        result fn start();
        result fn stop();
        result fn get_channels(num_input_channels: *mut i32, num_output_channels: *mut i32);
        result fn get_latencies(input_latency: *mut i32, output_latency: *mut i32);
        result fn get_buffer_size(min_size: *mut i32, max_size: *mut i32, preferred_size: *mut i32, granularity: *mut i32);
        result fn can_sample_rate(sample_rate: SampleRate);
        result fn get_sample_rate(sample_rate: *mut SampleRate);
        result fn set_sample_rate(sample_rate: SampleRate);
        result fn get_clock_sources(clocks: *mut ClockSource, num_sources: *mut i32);
        result fn set_clock_source(reference: i32);
        result fn get_sample_position(s_pos: *mut Samples, t_stamp: *mut TimeStamp);
        result fn get_channel_info(info: *mut ChannelInfo);
        result fn create_buffers(buffer_infos: *mut BufferInfo, num_channels: i32, buffer_size: i32, callbacks: *mut Callbacks);
        result fn dispose_buffers();
        result fn control_panel();
        result fn output_ready();
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

    unsafe extern "system" fn query_interface(
        this: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> Hresult {
        if ppv.is_null() {
            return E_POINTER;
        }

        let iid = *riid;
        if iid == IID_IUNKNOWN || iid == IID_ICLASSFACTORY {
            *ppv = this as *mut c_void;
            Self::add_ref(this);
            S_OK
        } else {
            *ppv = ptr::null_mut();
            E_NOINTERFACE
        }
    }

    unsafe extern "system" fn add_ref(this: *mut IUnknown) -> u32 {
        let factory = &*(this as *const ClassFactory<T>);
        factory.ref_count.fetch_add(1, Ordering::AcqRel) + 1
    }

    unsafe extern "system" fn release(this: *mut IUnknown) -> u32 {
        let factory = &*(this as *const ClassFactory<T>);
        let prev = factory.ref_count.fetch_sub(1, Ordering::AcqRel);

        if prev == 1 {
            drop(Box::from_raw(this as *mut ClassFactory<T>));
            0
        } else {
            prev - 1
        }
    }

    unsafe extern "system" fn create_instance(
        _this: *mut IClassFactory,
        outer: *mut IUnknown,
        riid: Refiid,
        ppv: *mut *mut c_void,
    ) -> Hresult {
        if ppv.is_null() {
            return E_POINTER;
        }

        *ppv = ptr::null_mut();

        if !outer.is_null() {
            return CLASS_E_NOAGGREGATION;
        }

        let object = AsioObject::<T>::new_raw();
        let unknown = object as *mut IUnknown;
        let vtbl = (*unknown).lp_vtbl;
        let hr = ((*vtbl).query_interface)(unknown, riid, ppv);
        ((*vtbl).release)(unknown);

        hr
    }

    unsafe extern "system" fn lock_server(_this: *mut IClassFactory, lock: Bool) -> Hresult {
        match lock {
            Bool::True => {
                SERVER_LOCKS.fetch_add(1, Ordering::Relaxed);
            }
            Bool::False => {
                SERVER_LOCKS.fetch_sub(1, Ordering::Relaxed);
            }
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
) -> Hresult {
    if ppv.is_null() {
        return E_POINTER;
    }

    *ppv = ptr::null_mut();

    if *rclsid != T::CLSID {
        return CLASS_E_CLASSNOTAVAILABLE;
    }

    let factory = ClassFactory::<T>::new_raw();
    let unknown = factory as *mut IUnknown;
    let vtbl = (*unknown).lp_vtbl;
    let hr = ((*vtbl).query_interface)(unknown, riid, ppv);
    ((*vtbl).release)(unknown);

    hr
}

pub fn dll_can_unload_now() -> Hresult {
    if OBJECT_COUNT.load(Ordering::Relaxed) == 0 && SERVER_LOCKS.load(Ordering::Relaxed) == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[macro_export]
macro_rules! export_asio_driver {
    ($t:ty) => {
        /// # Safety
        ///
        /// Called by COM runtime. `rclsid` and `riid` must be valid non-null `GUID`
        /// pointers. `ppv` must be null or a valid writable `*mut c_void` pointer.
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn DllGetClassObject(
            rclsid: $crate::asio::Refiid,
            riid: $crate::asio::Refiid,
            ppv: *mut *mut core::ffi::c_void,
        ) -> i32 { unsafe {
            $crate::com::dll_get_class_object::<$t>(rclsid, riid, ppv)
        }}

        /// # Safety
        ///
        /// Called by COM runtime to check if the DLL can be unloaded.
        #[unsafe(no_mangle)]
        pub unsafe extern "system" fn DllCanUnloadNow() -> i32 {
            $crate::com::dll_can_unload_now()
        }
    };
}
