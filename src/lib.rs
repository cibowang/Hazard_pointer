use std::ptr::NonNull;
use std::sync::atomic::AtomicPtr;
use std::sync::atomic::Ordering;

static SHARED_HAZPTR_LINKEDLIST: HazPtrLinkedlist = HazPtrLinkedlist;

#[derive(Default)]
pub struct HazPtrHolder(Option<*mut HazPtr>);

impl HazPtrHolder {
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
            hazptr.guard(ptr1);
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

pub struct HazPtr;

struct HazPtrLinkedlist;

struct Retire;

pub trait HazPtrObject {
    fn retire(me: *mut Self) {
        let _ = &SHARED_HAZPTR_LINKEDLIST;
    }
}

impl<T> HazPtrObject for T {}

mod tests {
    #[cfg(test)]
    use super::*;

    #[test]
    fn looks_good_to_me() {
        let x = AtomicPtr::new(Box::into_raw(Box::new(42)));
        let h = HazPtrHolder::default();
        let h_ref: i32 = h.load(&h);
        drop(h);
        // invalid
        let _ = *h_ref;

        let old = x.swap(
            Box::into_raw(Box::new(999)),
            std::sync::atomic::Ordering::SeqCst,
        );
        HazPtrObject::retire();
    }
}
