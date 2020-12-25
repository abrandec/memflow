/*!
32-bit Pointer abstraction.
*/

use crate::dataview::Pod;
use crate::error::PartialResult;
use crate::mem::VirtualMemory;
use crate::types::{Address, ByteSwap};

use std::convert::TryFrom;
use std::marker::PhantomData;
use std::mem::size_of;
use std::{cmp, fmt, hash, ops};

/// This type can be used in structs that are being read from the target memory.
/// It holds a phantom type that can be used to describe the proper type of the pointer
/// and to read it in a more convenient way.
///
/// This module is a direct adaption of [CasualX's great IntPtr crate](https://github.com/CasualX/intptr).
///
/// Generally the generic Type should implement the Pod trait to be read into easily.
/// See [here](https://docs.rs/dataview/0.1.1/dataview/) for more information on the Pod trait.
///
/// # Examples
///
/// ```
/// use memflow::types::Pointer32;
/// use memflow::mem::VirtualMemory;
/// use memflow::dataview::Pod;
///
/// #[repr(C)]
/// #[derive(Clone, Debug, Pod)]
/// struct Foo {
///     pub some_value: i32,
/// }
///
/// #[repr(C)]
/// #[derive(Clone, Debug, Pod)]
/// struct Bar {
///     pub foo_ptr: Pointer32<Foo>,
/// }
///
/// fn read_foo_bar<T: VirtualMemory>(virt_mem: &mut T) {
///     let bar: Bar = virt_mem.virt_read(0x1234.into()).unwrap();
///     let foo = bar.foo_ptr.deref(virt_mem).unwrap();
///     println!("value: {}", foo.some_value);
/// }
///
/// # use memflow::mem::dummy::DummyMemory;
/// # use memflow::types::size;
/// # read_foo_bar(&mut DummyMemory::new_virt(size::mb(4), size::mb(2), &[]).0);
///
/// ```
///
/// ```
/// use memflow::types::Pointer32;
/// use memflow::mem::VirtualMemory;
/// use memflow::dataview::Pod;
///
/// #[repr(C)]
/// #[derive(Clone, Debug, Pod)]
/// struct Foo {
///     pub some_value: i32,
/// }
///
/// #[repr(C)]
/// #[derive(Clone, Debug, Pod)]
/// struct Bar {
///     pub foo_ptr: Pointer32<Foo>,
/// }
///
/// fn read_foo_bar<T: VirtualMemory>(virt_mem: &mut T) {
///     let bar: Bar = virt_mem.virt_read(0x1234.into()).unwrap();
///     let foo = virt_mem.virt_read_ptr32(bar.foo_ptr).unwrap();
///     println!("value: {}", foo.some_value);
/// }
///
/// # use memflow::mem::dummy::DummyMemory;
/// # use memflow::types::size;
/// # read_foo_bar(&mut DummyMemory::new_virt(size::mb(4), size::mb(2), &[]).0);
/// ```
#[repr(transparent)]
#[cfg_attr(feature = "serde", derive(::serde::Serialize))]
pub struct Pointer32<T: ?Sized = ()> {
    pub address: u32,
    phantom_data: PhantomData<fn() -> T>,
}

impl<T: ?Sized> Pointer32<T> {
    const PHANTOM_DATA: PhantomData<fn() -> T> = PhantomData;

    /// A pointer32 with the value of zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// println!("pointer32: {}", Pointer32::<()>::NULL);
    /// ```
    pub const NULL: Pointer32<T> = Pointer32 {
        address: 0,
        phantom_data: PhantomData,
    };

    /// Returns a pointer32 with a value of zero.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// println!("pointer32: {}", Pointer32::<()>::null());
    /// ```
    #[inline]
    pub const fn null() -> Self {
        Pointer32::NULL
    }

    /// Checks wether the pointer32 is zero or not.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// assert_eq!(Pointer32::<()>::null().is_null(), true);
    /// assert_eq!(Pointer32::<()>::from(0x1000u32).is_null(), false);
    /// ```
    #[inline]
    pub const fn is_null(self) -> bool {
        self.address == 0
    }

    /// Converts the pointer32 to an Option that is None when it is null
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// assert_eq!(Pointer32::<()>::null().non_null(), None);
    /// assert_eq!(Pointer32::<()>::from(0x1000u32).non_null(), Some(Pointer32::from(0x1000)));
    /// ```
    #[inline]
    pub fn non_null(self) -> Option<Pointer32<T>> {
        if self.is_null() {
            None
        } else {
            Some(self)
        }
    }

    /// Converts the pointer32 into a `u32` value.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// let ptr = Pointer32::<()>::from(0x1000u32);
    /// let ptr_u32: u32 = ptr.as_u32();
    /// assert_eq!(ptr_u32, 0x1000);
    /// ```
    #[inline]
    pub const fn as_u32(self) -> u32 {
        self.address
    }

    /// Converts the pointer32 into a `u64` value.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// let ptr = Pointer32::<()>::from(0x1000u32);
    /// let ptr_u64: u64 = ptr.as_u64();
    /// assert_eq!(ptr_u64, 0x1000);
    /// ```
    #[inline]
    pub const fn as_u64(self) -> u64 {
        self.address as u64
    }

    /// Converts the pointer32 into a `usize` value.
    ///
    /// # Examples
    ///
    /// ```
    /// use memflow::types::Pointer32;
    ///
    /// let ptr = Pointer32::<()>::from(0x1000u32);
    /// let ptr_usize: usize = ptr.as_usize();
    /// assert_eq!(ptr_usize, 0x1000);
    /// ```
    #[inline]
    pub const fn as_usize(self) -> usize {
        self.address as usize
    }

    /// Returns the underlying raw u32 value of this pointer.
    #[deprecated = "use as_u32() instead"]
    pub const fn into_raw(self) -> u32 {
        self.address
    }
}

/// This function will deref the pointer directly into a Pod type.
impl<T: Pod + ?Sized> Pointer32<T> {
    pub fn deref_into<U: VirtualMemory>(self, mem: &mut U, out: &mut T) -> PartialResult<()> {
        mem.virt_read_ptr32_into(self, out)
    }
}

/// This function will return the Object this pointer is pointing towards.
impl<T: Pod + Sized> Pointer32<T> {
    pub fn deref<U: VirtualMemory>(self, mem: &mut U) -> PartialResult<T> {
        mem.virt_read_ptr32(self)
    }
}

impl<T> Pointer32<[T]> {
    pub const fn decay(self) -> Pointer32<T> {
        Pointer32 {
            address: self.address,
            phantom_data: Pointer32::<T>::PHANTOM_DATA,
        }
    }

    pub const fn at(self, i: usize) -> Pointer32<T> {
        let address = self.address + (i * size_of::<T>()) as u32;
        Pointer32 {
            address,
            phantom_data: Pointer32::<T>::PHANTOM_DATA,
        }
    }
}

impl<T: ?Sized> Copy for Pointer32<T> {}
impl<T: ?Sized> Clone for Pointer32<T> {
    #[inline(always)]
    fn clone(&self) -> Pointer32<T> {
        *self
    }
}
impl<T: ?Sized> Default for Pointer32<T> {
    #[inline(always)]
    fn default() -> Pointer32<T> {
        Pointer32::NULL
    }
}
impl<T: ?Sized> Eq for Pointer32<T> {}
impl<T: ?Sized> PartialEq for Pointer32<T> {
    #[inline(always)]
    fn eq(&self, rhs: &Pointer32<T>) -> bool {
        self.address == rhs.address
    }
}
impl<T: ?Sized> PartialOrd for Pointer32<T> {
    #[inline(always)]
    fn partial_cmp(&self, rhs: &Pointer32<T>) -> Option<cmp::Ordering> {
        self.address.partial_cmp(&rhs.address)
    }
}
impl<T: ?Sized> Ord for Pointer32<T> {
    #[inline(always)]
    fn cmp(&self, rhs: &Pointer32<T>) -> cmp::Ordering {
        self.address.cmp(&rhs.address)
    }
}
impl<T: ?Sized> hash::Hash for Pointer32<T> {
    #[inline(always)]
    fn hash<H: hash::Hasher>(&self, state: &mut H) {
        self.address.hash(state)
    }
}
impl<T: ?Sized> AsRef<u32> for Pointer32<T> {
    #[inline(always)]
    fn as_ref(&self) -> &u32 {
        &self.address
    }
}
impl<T: ?Sized> AsMut<u32> for Pointer32<T> {
    #[inline(always)]
    fn as_mut(&mut self) -> &mut u32 {
        &mut self.address
    }
}

// From implementations
impl<T: ?Sized> From<u32> for Pointer32<T> {
    #[inline(always)]
    fn from(address: u32) -> Pointer32<T> {
        Pointer32 {
            address,
            phantom_data: PhantomData,
        }
    }
}

/// Tries to converts an u64 into a Pointer32.
/// The function will return an `Error::Bounds` error if the input value is greater than `u32::max_value()`.
impl<T: ?Sized> TryFrom<u64> for Pointer32<T> {
    type Error = crate::error::Error;

    fn try_from(address: u64) -> Result<Pointer32<T>, Self::Error> {
        if address <= (u32::max_value() as u64) {
            Ok(Pointer32 {
                address: address as u32,
                phantom_data: PhantomData,
            })
        } else {
            Err(crate::error::Error::Bounds)
        }
    }
}

/// Tries to converts an Address into a Pointer32.
/// The function will return an Error::Bounds if the input value is greater than `u32::max_value()`.
impl<T: ?Sized> TryFrom<Address> for Pointer32<T> {
    type Error = crate::error::Error;

    fn try_from(address: Address) -> Result<Pointer32<T>, Self::Error> {
        if address.as_u64() <= (u32::max_value() as u64) {
            Ok(Pointer32 {
                address: address.as_u32(),
                phantom_data: PhantomData,
            })
        } else {
            Err(crate::error::Error::Bounds)
        }
    }
}

// Into implementations
impl<T: ?Sized> From<Pointer32<T>> for Address {
    #[inline(always)]
    fn from(ptr: Pointer32<T>) -> Address {
        ptr.address.into()
    }
}

impl<T: ?Sized> From<Pointer32<T>> for u32 {
    #[inline(always)]
    fn from(ptr: Pointer32<T>) -> u32 {
        ptr.address
    }
}

impl<T: ?Sized> From<Pointer32<T>> for u64 {
    #[inline(always)]
    fn from(ptr: Pointer32<T>) -> u64 {
        ptr.address as u64
    }
}

// Arithmetic operations
impl<T> ops::Add<usize> for Pointer32<T> {
    type Output = Pointer32<T>;
    #[inline(always)]
    fn add(self, other: usize) -> Pointer32<T> {
        let address = self.address + (other * size_of::<T>()) as u32;
        Pointer32 {
            address,
            phantom_data: self.phantom_data,
        }
    }
}
impl<T> ops::Sub<usize> for Pointer32<T> {
    type Output = Pointer32<T>;
    #[inline(always)]
    fn sub(self, other: usize) -> Pointer32<T> {
        let address = self.address - (other * size_of::<T>()) as u32;
        Pointer32 {
            address,
            phantom_data: self.phantom_data,
        }
    }
}

impl<T: ?Sized> fmt::Debug for Pointer32<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.address)
    }
}
impl<T: ?Sized> fmt::UpperHex for Pointer32<T> {
    #[inline(always)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:X}", self.address)
    }
}
impl<T: ?Sized> fmt::LowerHex for Pointer32<T> {
    #[inline(always)]
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.address)
    }
}
impl<T: ?Sized> fmt::Display for Pointer32<T> {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{:x}", self.address)
    }
}

unsafe impl<T: ?Sized + 'static> Pod for Pointer32<T> {}
const _: [(); std::mem::size_of::<Pointer32<()>>()] = [(); std::mem::size_of::<u32>()];

impl<T: ?Sized + 'static> ByteSwap for Pointer32<T> {
    fn byte_swap(&mut self) {
        self.address.byte_swap();
    }
}
