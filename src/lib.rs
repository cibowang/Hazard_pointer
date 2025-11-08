#![feature(arbitrary_self_types_pointers)]
#![deny(unsafe_op_in_unsafe_fn)]

use std::mem::needs_drop;
use std::ops::Deref;
use std::ops::DerefMut;
use std::ptr::NonNull;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;

static SHARED_HAZPTR_LINKEDLIST: HazPtrLinkedlist = HazPtrLinkedlist {
    hazptrs: HazPtr,
    retired: Retired,
};

struct HazPtrLinkedlist {
    hazptrs: HazPtr,
    retired: Retired,
}

impl HazPtrLinkedlist {
    pub fn acquire(&self) -> &'static HazPtr {
        todo!()
    }
    pub fn retire<D: Deleter>(&self, _ptr: *mut dyn Drop) {}
}

pub struct HazPtr {
    ptr: AtomicPtr<()>,
    next: AtomicPtr<HazPtr>,
    active: AtomicBool,
}

impl HazPtr {
    pub fn guard(&self, ptr: *mut ()) {
        self.ptr.store(ptr, Ordering::SeqCst)
    }
}

pub trait Deleter {
    fn delete(_ptr: *mut dyn Drop) {}
}

pub trait HazPtrObject<D>
where
    Self: Sized + Drop + 'static,
{
    /// # Safety
    ///
    /// Caller must guarantee that the pointer is a valid reference.
    /// Caller must guarantee that Self is no longer accessible to readers.
    /// It is Okay for existing readers to still refer to Self.
    unsafe fn retire<D: Deleter>(self: *mut Self) {
        if !needs_drop::<Self>() {
            return;
        }
        unsafe { &*self }
            .linkedlist()
            .retire::<D>(self as *mut dyn Drop);
    }
    fn linkedlist(&self) -> &HazPtrLinkedlist;
}

pub struct HazPtrWrapper<T> {
    inner: T,
    // linkedlist: *const SHARED_HAZPTR_LINKEDLIST,
}

impl<T> HazPtrWrapper<T> {
    pub fn with_default_linkedlist(t: T) -> Self {
        Self { inner: t }
    }
}

impl<T: 'static, D> HazPtrObject<D> for HazPtrWrapper<T> {
    fn linkedlist(&self) -> &HazPtrLinkedlist {
        &SHARED_HAZPTR_LINKEDLIST
    }
}

impl<T> Drop for HazPtrWrapper<T> {
    fn drop(&mut self) {}
}

impl<T> Deref for HazPtrWrapper<T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<T> DerefMut for HazPtrWrapper<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

#[derive(Default)]
pub struct HazPtrHolder(Option<&'static HazPtr>);

impl HazPtrHolder {
    fn hazptr(&mut self) -> &'static HazPtr {
        if let Some(hazptr) = self.0 {
            hazptr
        } else {
            let hazptr = SHARED_HAZPTR_LINKEDLIST.acquire();
            hazptr
        }
    }
    /// # Safety
    ///
    /// Caller must guarantee that address in AtomicPtr is a valid reference.
    /// Caller must also guarantee that the value behind the AtomicPtr will only be dellocated
    /// through calls to ['HazPtrObj::retire'].
    pub unsafe fn load<T>(&mut self, ptr: &'_ AtomicPtr<T>) -> Option<&T> {
        let hazptr = if let Some(hazptr) = self.0 {
            hazptr
        } else {
            let hazptr = SHARED_HAZPTR_LINKEDLIST.acquire();
            self.0 = Some(hazptr);
            hazptr
        };
        loop {
            let mut ptr1 = ptr.load(Ordering::SeqCst);
            hazptr.guard(ptr1 as *mut ());
            let ptr2 = ptr.load(Ordering::SeqCst);
            if ptr1 == ptr2 {
                break NonNull::new(ptr1).map(|nn| {
                    // Safety: this is safe because:
                    //
                    // 1: Since hazard pointer is active and pointing at ptr1, it's memory will not
                    //    be deallocated for the return lifetime.
                    // 2: The pointer address is valid by the safety contract of load.
                    unsafe { nn.as_ref() }
                });
            } else {
                ptr1 = ptr2;
            }
        }
    }
}

impl Drop for HazPtrHolder {
    fn drop(&mut self) {
        if let Some(hazptr) = self.0 {
            hazptr.active.store(false, Ordering::SeqCst);
        }
    }
}

struct Retired;

mod tests {
    #[cfg(test)]
    use super::*;

    #[test]
    fn looks_good_to_me() {
        let x = AtomicPtr::new(Box::into_raw(Box::new(42)));
        let mut h = HazPtrHolder::default();
        // Safety:
        //
        // 1. AtomicPtr points to a Box, which is always valid.
        // 2. Writers to AtomicPtr use HazPtrObj::retire.
        let h_ref = unsafe { h.load(&h) }.expect("not null");
        drop(h);
        // invalid
        let _ = *h_ref;

        let old = x.swap(
            Box::into_raw(Box::new(HazPtrWrapper::with_default_linkedlist(999))),
            std::sync::atomic::Ordering::SeqCst,
        );
        old.retire();
    }
}
