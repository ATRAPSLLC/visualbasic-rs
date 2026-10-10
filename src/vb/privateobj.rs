//! PrivateObjectDescriptor structure parser.
//!
//! The PrivateObjectDescriptor contains per-object private data including
//! function type descriptors, its public variables and implemented
//! interfaces, its events' prototypes and the layouts of its `Type`s.
//! It is referenced by [`ObjectInfo::private_object_va()`](super::object::ObjectInfo::private_object_va)
//! at offset 0x0C.
//!
//! # Layout (0x40 bytes)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 4 | Reserved (0 in every fixture) |
//! | 0x04 | 4 | `lpObjectInfo` - back-pointer to parent ObjectInfo |
//! | 0x08 | 4 | Reserved (0xFFFFFFFF in every fixture) |
//! | 0x0C | 4 | Reserved (0 in every fixture) |
//! | 0x10 | 2 | Module-level variables, plus 1 per `Implements` (see [`member_count`](PrivateObjectDescriptor::member_count)) |
//! | 0x12 | 2 | `Event` declarations (see [`event_count`](PrivateObjectDescriptor::event_count)) |
//! | 0x14 | 2 | `Type` declarations (see [`record_layout_count`](PrivateObjectDescriptor::record_layout_count)) |
//! | 0x16 | 2 | Padding (0 in every fixture) |
//! | 0x18 | 4 | `lpFuncTypDescs` - VA to array of FuncTypDesc pointers, one per method (null for the unnamed ones) |
//! | 0x1C | 4 | `lpExtendedFuncData` - secondary per-method array (0 in every fixture) |
//! | 0x20 | 4 | `lpMemberDescs` - VA to array of [`MemberDesc`](super::member::MemberDesc) pointers, one per +0x10 member (null for a `Private` variable) |
//! | 0x24 | 4 | `lpEventDescs` - VA to array of FuncTypDesc pointers, one per +0x12 `Event` |
//! | 0x28 | 4 | VA to array of [`RecordLayout`](super::controlprop::RecordLayout) pointers, one per +0x14 `Type` |
//! | 0x2C | 12 | Reserved (0 in every fixture) |
//! | 0x38 | 4 | Instance size (equal to the PublicBytes `+0x02`; see [`instance_size`](PrivateObjectDescriptor::instance_size)) |
//! | 0x3C | 4 | `dwFlags` - 0x0104 for classes and UserControls, 0x0004 for forms |
//!
//! Standard modules have no PrivateObjectDescriptor
//! (`ObjectInfo.private_object_va` is 0xFFFFFFFF in every fixture module).
//!
//! MSVBVM60 6.00.9848 reads the +0x18 array at `0x660F63ED` (with the
//! method names of the PublicObjectDescriptor). Its function at
//! `0x660F6349` indexes both the +0x18 and the +0x1C array and
//! dereferences the +0x1C one unconditionally (`0x660F6390`), so it never
//! runs on a descriptor like the fixtures', whose +0x1C is 0.

use crate::{
    error::Error,
    util::{read_u16_le, read_u32_le},
};

/// View over a PrivateObjectDescriptor structure (0x40 bytes).
///
/// Contains per-object private data: function type descriptor pointers,
/// member and event descriptor pointers, and their counts.
///
/// # Accessor fallibility
///
/// [`parse`](Self::parse) validates that the fixed 0x40-byte header is
/// present. After that, fixed-offset accessors on this type are only
/// fallible if the already-validated backing slice is unexpectedly too
/// short or arithmetic overflows while reading primitive fields. Methods
/// that only inspect those primitive fields, such as [`is_class`](Self::is_class),
/// do not follow VAs and treat unreadable fields as false predicates.
#[derive(Clone, Copy, Debug)]
pub struct PrivateObjectDescriptor<'a> {
    bytes: &'a [u8],
}

impl<'a> PrivateObjectDescriptor<'a> {
    /// Size of the structure in bytes.
    pub const SIZE: usize = 0x40;

    /// Parses a PrivateObjectDescriptor from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x40`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::SIZE {
            return Err(Error::TooShort {
                expected: Self::SIZE,
                actual: data.len(),
                context: "PrivateObjectDescriptor",
            });
        }
        let bytes = data.get(..Self::SIZE).ok_or(Error::TooShort {
            expected: Self::SIZE,
            actual: data.len(),
            context: "PrivateObjectDescriptor",
        })?;
        Ok(Self { bytes })
    }

    /// Back-pointer to the parent [`ObjectInfo`](super::object::ObjectInfo) at offset 0x04.
    #[inline]
    pub fn object_info_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x04)
    }

    /// The u16 count at offset 0x10: module-level variables plus
    /// implemented interfaces.
    ///
    /// The number of module-level variable declarations (`Private`,
    /// `Public`, `WithEvents`; not `Static` locals, not controls), plus 1
    /// per `Implements`. It holds for the 16 objects checked against their
    /// source in `tests/fixtures`: `calls` Counter 3 (`m_Value`, `m_Scale`,
    /// `m_Owner`), Square 2 (`m_Side`, `Implements Shape`), Shape 0;
    /// `events` Listener 2 (`WithEvents m_Source`, `m_Log`); `data` Item 1
    /// (`Public Name`). It is not a method count: the FuncTypDesc array has
    /// one entry per method of the object's method table. It is the length
    /// of the [`member_descs_va`](Self::member_descs_va) array.
    #[inline]
    pub fn member_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x10)
    }

    /// Number of `Event` declarations, the u16 at offset 0x12.
    ///
    /// `calls` Counter 1,
    /// `events` Source 3 (`Started`, `Ticked`, `Named`), `types` Kinds 1,
    /// `forms` Gauge 1, 0 for every other fixture object. It is the length
    /// of the [`event_descs_va`](Self::event_descs_va) array.
    #[inline]
    pub fn event_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x12)
    }

    /// Number of `Type` declarations in the object, `Public` or `Private`,
    /// the u16 at offset 0x14: the length of the
    /// [`record_layouts_va`](Self::record_layouts_va) array.
    ///
    /// `typerefs` `Shapes` 1 (`Point`), `records` `Item` 1 (`CT`), `udts`
    /// `Shapes` 5; 0 for an object that declares none. A standard module's
    /// `Type`s are in no such array: a module has no descriptor.
    #[inline]
    pub fn record_layout_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x14)
    }

    /// VA of the [`FuncTypDesc`](super::functype::FuncTypDesc) pointer array at offset 0x18.
    ///
    /// Points to an array of VAs, one per method of the object's method
    /// table. Each VA points to a
    /// [`FuncTypDesc`](super::functype::FuncTypDesc) structure; pass
    /// [`FuncTypDesc::parse`](super::functype::FuncTypDesc::parse) the bytes
    /// to the end of the section for its type list. Null entries (VA == 0)
    /// are the methods with no name in
    /// [`PublicObjectDescriptor::method_names_va`](super::object::PublicObjectDescriptor::method_names_va):
    /// `Private` and `Friend` procedures, `Class_Initialize`/`Terminate`
    /// and a form's control event handlers. A class's `WithEvents`
    /// handlers have one (`events` `Listener.m_Source_Started`).
    #[inline]
    pub fn func_type_descs_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x18)
    }

    /// VA of the [`MemberDesc`](super::member::MemberDesc) pointer array at
    /// offset 0x20.
    ///
    /// One VA per [`member_count`](Self::member_count): a `MemberDesc` for a
    /// `Public` variable (`WithEvents` or not) or an `Implements`, 0 for a
    /// `Private` variable. `members` `Holder` points to its thirteen
    /// `Public` variables `B` ... `Auto`, `calls` `Square` to 0 (`m_Side`)
    /// and `Shape` (its `Implements`). See [`member`](super::member) for the
    /// order. With no members it shares its VA with
    /// [`event_descs_va`](Self::event_descs_va) and
    /// [`record_layouts_va`](Self::record_layouts_va) (`calls` Shape: all
    /// three 0x00401C58).
    #[inline]
    pub fn member_descs_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x20)
    }

    /// VA of the event prototype array at offset 0x24.
    ///
    /// One [`FuncTypDesc`](super::functype::FuncTypDesc) VA per
    /// [`event_count`](Self::event_count), in `Event` declaration order;
    /// each descriptor's parameter names are the event's (`calls` Counter:
    /// `Changed(ByVal value)`; `events` Source: `Started`, `Ticked(n, ratio,
    /// Cancel)`, `Named(s, v, o)`).
    #[inline]
    pub fn event_descs_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x24)
    }

    /// VA of the record layout array at offset 0x28.
    ///
    /// One [`RecordLayout`](super::controlprop::RecordLayout) VA per
    /// [`record_layout_count`](Self::record_layout_count), in `Type`
    /// declaration order (`udts` `Shapes`: `Point`, `Named`, `Third`,
    /// `PrivT`, `Holder`). The compiler stores one layout per distinct shape
    /// in the project, so two `Type`s with the same members share one
    /// (`udts`: `Second.Pair` and `Shapes.PrivT`), as does an array
    /// member's element (`Holder.Ns()` names `Named`'s).
    #[inline]
    pub fn record_layouts_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x28)
    }

    /// Size of an instance of the object, at offset 0x38.
    ///
    /// Equal to [`ClassFormPublicBytes::instance_size`](super::publicbytes::ClassFormPublicBytes::instance_size)
    /// (PublicBytes `+0x02`) for all 51 objects with a descriptor in
    /// `tests/fixtures` (`calls` Shape 0x40, Counter 0x58; `dispid` Bag
    /// 0x88, Dial 0x94). It is not the size of the FuncTypDesc area.
    #[inline]
    pub fn instance_size(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x38)
    }

    /// Object flags at offset 0x3C.
    ///
    /// Values in `tests/fixtures`:
    /// - `0x0004`: forms (`controls` Form1, `dispid` Host, `forms` Board).
    /// - `0x0104`: classes and UserControls (`dispid` Dial, `forms` Gauge).
    ///
    /// Bit `0x0004` is set on every descriptor; bit `0x0100` on the objects
    /// whose `fObjectType` low byte is `0x03`.
    #[inline]
    pub fn flags(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x3C)
    }

    /// Returns `true` if bit `0x0100` of the flags at +0x3C is set.
    ///
    /// Set for class modules and UserControls alike, the same objects
    /// [`PublicObjectDescriptor::is_class()`](super::object::PublicObjectDescriptor::is_class)
    /// accepts (bit `0x02` set, bit `0x80` clear in `fObjectType`).
    #[inline]
    pub fn is_class(&self) -> bool {
        self.flags().is_ok_and(|f| f & 0x0100 != 0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Real data from Cls_Zip in pe_x86_vb_loader sample
    const CLS_ZIP: [u8; 0x40] = [
        0x00, 0x00, 0x00, 0x00, 0x1C, 0x28, 0x40, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
        0x00, 0x0E, 0x00, 0x00, 0x00, 0x05, 0x00, 0x00, 0x00, 0xA8, 0x5C, 0x40, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x30, 0x5B, 0x40, 0x00, 0x44, 0x55, 0x40, 0x00, 0x44, 0x56, 0x40, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x00, 0x00, 0x00,
        0x04, 0x01, 0x00, 0x00,
    ];

    // Real data from Form1 in pe_x86_vb_loader sample
    const FORM1: [u8; 0x40] = [
        0x00, 0x00, 0x00, 0x00, 0x3C, 0x24, 0x40, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x8C, 0x55, 0x40, 0x00, 0x00, 0x00,
        0x00, 0x00, 0x44, 0x55, 0x40, 0x00, 0x44, 0x55, 0x40, 0x00, 0x44, 0x55, 0x40, 0x00, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x44, 0x00, 0x00, 0x00,
        0x04, 0x00, 0x00, 0x00,
    ];

    #[test]
    fn test_parse_cls_zip() {
        let pod = PrivateObjectDescriptor::parse(&CLS_ZIP).unwrap();
        assert_eq!(pod.object_info_va().unwrap(), 0x0040281C);
        assert_eq!(pod.member_count().unwrap(), 14);
        assert_eq!(pod.record_layout_count().unwrap(), 5);
        assert_eq!(pod.func_type_descs_va().unwrap(), 0x00405CA8);
        assert_eq!(pod.event_descs_va().unwrap(), 0x00405544);
        assert_eq!(pod.record_layouts_va().unwrap(), 0x00405644);
        assert_eq!(pod.instance_size().unwrap(), 0x4C);
        assert_eq!(pod.flags().unwrap(), 0x0104);
        assert!(pod.is_class());
    }

    #[test]
    fn test_parse_form1() {
        let pod = PrivateObjectDescriptor::parse(&FORM1).unwrap();
        assert_eq!(pod.object_info_va().unwrap(), 0x0040243C);
        assert_eq!(pod.member_count().unwrap(), 0);
        assert_eq!(pod.record_layout_count().unwrap(), 0);
        assert_eq!(pod.func_type_descs_va().unwrap(), 0x0040558C);
        assert_eq!(pod.instance_size().unwrap(), 0x44);
        assert_eq!(pod.flags().unwrap(), 0x0004);
        assert!(!pod.is_class());
    }

    // Real data from CoolBar in ComCt332.ocx - event_count is non-zero (13)
    const COOLBAR_OCX: [u8; 0x40] = [
        0x00, 0x00, 0x00, 0x00, 0x84, 0x71, 0x08, 0x28, 0xFF, 0xFF, 0xFF, 0xFF, 0x00, 0x00, 0x00,
        0x00, 0x1E, 0x00, 0x0D, 0x00, 0x00, 0x00, 0x00, 0x00, 0x4C, 0x25, 0x09, 0x28, 0x00, 0x00,
        0x00, 0x00, 0x3C, 0x22, 0x09, 0x28, 0xCC, 0x1D, 0x09, 0x28, 0x50, 0x05, 0x09, 0x28, 0x00,
        0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xBC, 0x00, 0x00, 0x00,
        0x04, 0x01, 0x00, 0x00,
    ];

    #[test]
    fn test_parse_coolbar_ocx() {
        let pod = PrivateObjectDescriptor::parse(&COOLBAR_OCX).unwrap();
        assert_eq!(pod.member_count().unwrap(), 30);
        assert_eq!(pod.event_count().unwrap(), 13);
        assert_eq!(pod.record_layout_count().unwrap(), 0);
        assert_eq!(pod.flags().unwrap(), 0x0104);
        assert!(pod.is_class());
    }

    #[test]
    fn test_parse_too_short() {
        let short = [0u8; 0x3F];
        assert!(PrivateObjectDescriptor::parse(&short).is_err());
    }
}
