extern crate lazy_static;
extern crate once_cell;

use lazy_static::lazy_static;
use once_cell::sync::Lazy;
use serde::{Deserialize, Serialize};
use std::any::{type_name, Any};
use std::borrow::Cow;
use std::cell::{Cell, RefCell, UnsafeCell};
use std::char;
use std::cmp::{max, min, Eq, Ord, PartialEq, PartialOrd};
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::convert::{identity, TryFrom, TryInto};
use std::ffi::{CStr, CString, OsStr, OsString};
use std::fmt::{self, Debug, Display, Formatter};
use std::fs::File;
use std::hint::unreachable_unchecked;
use std::io;
use std::io::prelude::*;
use std::iter::{self, FromIterator};
use std::marker::PhantomData;
use std::mem::{align_of, align_of_val, needs_drop, size_of, size_of_val};
use std::mem::{forget, replace, swap, take, transmute, transmute_copy};
use std::mem::{ManuallyDrop, MaybeUninit};
use std::num::{NonZeroI128, NonZeroI16, NonZeroI32, NonZeroI64, NonZeroI8, NonZeroIsize};
use std::num::{NonZeroU128, NonZeroU16, NonZeroU32, NonZeroU64, NonZeroU8, NonZeroUsize};
use std::ops::*;
use std::path::{Path, PathBuf};
use std::ptr::{self, addr_of, addr_of_mut, null, null_mut, NonNull};
use std::rc::Rc;
use std::slice;
use std::str;
use std::sync::atomic::{self, AtomicBool, AtomicPtr};
use std::sync::atomic::{AtomicI16, AtomicI32, AtomicI64, AtomicI8, AtomicIsize};
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, AtomicU8, AtomicUsize};
use std::sync::{Arc, Mutex, RwLock};

fn type_name_of_val<T: ?Sized>(_: &T) -> &'static str {
    type_name::<T>()
}
