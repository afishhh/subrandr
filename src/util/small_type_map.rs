use std::{any::TypeId, collections::HashMap, fmt::Debug, mem::MaybeUninit, ptr::drop_in_place};

pub struct SmallTypeMap<const MAX_VALUE_SIZE: usize> {
    values: HashMap<TypeId, ValueSlot<MAX_VALUE_SIZE>>,
}

struct ValueVTable {
    name: &'static str,
    clone: unsafe fn(*const u8, *mut u8),
    debug: unsafe fn(*const u8, &mut std::fmt::Formatter) -> std::fmt::Result,
    drop_in_place: unsafe fn(*mut u8),
}

struct ValueSlot<const MAX_VALUE_SIZE: usize> {
    data: [MaybeUninit<u8>; MAX_VALUE_SIZE],
    vtable: &'static ValueVTable,
}

impl<const MAX_VALUE_SIZE: usize> ValueSlot<MAX_VALUE_SIZE> {
    fn new_uninit(vtable: &'static ValueVTable) -> Self {
        Self {
            data: [const { MaybeUninit::<u8>::uninit() }; MAX_VALUE_SIZE],
            vtable,
        }
    }

    fn write_data<T>(&mut self, value: T) {
        const { assert!(std::mem::size_of::<T>() < MAX_VALUE_SIZE) };

        unsafe {
            let ptr = &value as *const _ as *const u8;

            let out_ptr = self.data.as_mut_ptr().cast();
            std::ptr::copy(ptr, out_ptr, std::mem::size_of::<T>());

            std::mem::forget(value);
        }
    }
}

unsafe fn clone_raw_value<T: Clone + 'static>(original: *const u8, result: *mut u8) {
    unsafe { result.cast::<T>().write((*original.cast::<T>()).clone()) }
}

unsafe fn debug_raw_value<T: Debug + 'static>(
    value: *const u8,
    fmt: &mut std::fmt::Formatter,
) -> std::fmt::Result {
    unsafe { (*value.cast::<T>()).fmt(fmt) }
}

impl<const MAX_VALUE_SIZE: usize> SmallTypeMap<MAX_VALUE_SIZE> {
    pub fn new() -> Self {
        Self {
            values: HashMap::new(),
        }
    }

    pub fn set<V: Debug + Clone + 'static>(&mut self, value: V) {
        let vtable: &'static ValueVTable = const {
            &ValueVTable {
                // FIXME: std::any::type_name is not yet stable in const...
                name: "PLACEHOLDER",
                clone: clone_raw_value::<V>,
                debug: debug_raw_value::<V>,
                drop_in_place: |value| unsafe { drop_in_place::<V>(value.cast()) },
            }
        };

        self.values
            .entry(TypeId::of::<V>())
            .insert_entry(ValueSlot::new_uninit(vtable))
            .into_mut()
            .write_data(value);
    }

    pub fn get<V: 'static>(&self) -> Option<&V> {
        self.values
            .get(&TypeId::of::<V>())
            .map(|erased| unsafe { &*erased.data.as_ptr().cast() })
    }

    pub fn get_copy_or_default<V: Default + Copy + 'static>(&self) -> V {
        match self.get::<V>() {
            Some(value) => *value,
            None => V::default(),
        }
    }

    pub fn merge(&mut self, other: &Self) {
        for (key, slot) in other.values.iter() {
            let new_slot = self
                .values
                .entry(*key)
                .insert_entry(ValueSlot::new_uninit(slot.vtable))
                .into_mut();
            unsafe {
                (slot.vtable.clone)(slot.data.as_ptr().cast(), new_slot.data.as_mut_ptr().cast());
            }
        }
    }
}

impl<const MAX_VALUE_SIZE: usize> Clone for SmallTypeMap<MAX_VALUE_SIZE> {
    fn clone(&self) -> Self {
        let mut result = Self::new();
        result.merge(self);
        result
    }
}

impl<const MAX_VALUE_SIZE: usize> Debug for SmallTypeMap<MAX_VALUE_SIZE> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut dbg = f.debug_map();
        for value in self.values.values() {
            dbg.key(&value.vtable.name);

            struct VtableDebug(
                *const u8,
                unsafe fn(*const u8, &mut std::fmt::Formatter) -> std::fmt::Result,
            );
            impl Debug for VtableDebug {
                fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                    unsafe { (self.1)(self.0, f) }
                }
            }

            dbg.value(&VtableDebug(value.data.as_ptr().cast(), value.vtable.debug));
        }

        dbg.finish()
    }
}

impl<const MAX_VALUE_SIZE: usize> Drop for SmallTypeMap<MAX_VALUE_SIZE> {
    fn drop(&mut self) {
        for value in self.values.values_mut() {
            unsafe { (value.vtable.drop_in_place)(value.data.as_mut_ptr().cast()) }
        }
    }
}

#[cfg(test)]
mod test {
    use super::SmallTypeMap;

    #[test]
    fn copy_values() {
        let mut map: SmallTypeMap<10> = SmallTypeMap::new();

        map.set::<i32>(64);
        map.set::<i64>(10);
        map.set::<u32>(13);
        map.set::<[u8; 5]>([1, 2, 3, 4, 5]);

        assert_eq!(map.get::<i64>(), Some(&10));
        assert_eq!(map.get::<i32>(), Some(&64));
        assert_eq!(map.get::<u32>(), Some(&13));
        assert_eq!(map.get::<[u8; 5]>(), Some(&[1, 2, 3, 4, 5]));
        assert_eq!(map.get::<u64>(), None);
    }

    #[test]
    fn clone_values() {
        let mut map: SmallTypeMap<64> = SmallTypeMap::new();

        map.set::<String>("hello".into());
        map.set::<u32>(13);
        map.set::<Box<str>>("world".into());

        assert_eq!(map.get::<i64>(), None);
        assert_eq!(map.get::<u32>(), Some(&13));
        assert_eq!(map.get::<String>().map(|x| &**x), Some("hello"));
        assert_eq!(map.get::<Box<str>>().map(|x| &**x), Some("world"));

        let clone = map.clone();

        assert_eq!(clone.get::<i64>(), None);
        assert_eq!(clone.get::<u32>(), Some(&13));
        assert_eq!(clone.get::<String>().map(|x| &**x), Some("hello"));
        assert_eq!(clone.get::<Box<str>>().map(|x| &**x), Some("world"));

        dbg!(clone);
    }
}
