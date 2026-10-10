//! Member descriptors: an object's public variables and implemented
//! interfaces.
//!
//! [`PrivateObjectDescriptor::member_descs_va`](super::privateobj::PrivateObjectDescriptor::member_descs_va)
//! (+0x20) points to an array of
//! [`member_count`](super::privateobj::PrivateObjectDescriptor::member_count)
//! VAs, one per module-level variable and per `Implements`. The entry of a
//! `Public` variable or an `Implements` points to a [`MemberDesc`]; the entry
//! of a `Private` variable (`WithEvents` or not) is 0.
//!
//! # Order
//!
//! The order of the object's vtable groups (see
//! [`MethodLink`](crate::project::MethodLink)): the variables in declaration
//! order (`Private` ones as null entries), then the `Implements`, then the
//! `WithEvents` variables. `members` `Mixed` declares `Value`,
//! `Implements IFirst`, `Public WithEvents Shown`, `Private WithEvents
//! Hidden`, `Note`, and its array is `Value`, `Note`, `IFirst`, `Shown`, 0;
//! `calls` `Square` declares `Implements Shape` before `Private m_Side`, and
//! its array is 0, `Shape`.
//!
//! # Layout (0x1C bytes, 0x20 with a type reference)
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 4 | `lpName` - VA of the null-terminated name |
//! | 0x04 | 4 | Reserved (0 in every fixture) |
//! | 0x08 | 4 | 0xFFFFFFFF for an `Implements` or a `WithEvents` variable, 0 otherwise |
//! | 0x0C | 4 | `memid` - the variable's DISPID (`0x40030000` + index); 0xFFFFFFFF for an `Implements` |
//! | 0x10 | 2 | Flags (2 in every fixture) |
//! | 0x12 | 2 | vtable byte offset of the variable's first accessor; 0xFFFF for an `Implements` |
//! | 0x14 | 4 | Byte offset of the variable in the instance; 0xFFFFFFFF for an `Implements` |
//! | 0x18 | 4 | Type code, an [`ArgType`] code |
//! | 0x1C | 4 | The type's descriptor VA, present only for the [`ArgType`] codes that carry one |
//!
//! `members` `Holder` declares thirteen `Public` variables `B As Byte` ...
//! `Auto As New Collection`: their memids are `0x40030000` to `0x4003000C`,
//! their accessors at vtable 0x1C (`B`) to 0x88 (`Auto`), their instance
//! offsets 0x34 (`B`) to 0x78 (`Auto`), the offsets the accessors'
//! [`MethodLinkKind::Variable`](crate::project::MethodLinkKind::Variable)
//! entries add.

use crate::{
    addressmap::AddressMap,
    error::Error,
    util::{read_cstr, read_u16_le, read_u32_le},
    vb::functype::ArgType,
};

/// What a [`MemberDesc`] declares.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberKind {
    /// A `Public` variable.
    Variable,
    /// A `Public WithEvents` variable.
    WithEvents,
    /// An `Implements` statement; the [`MemberDesc::var_type`] names the
    /// implemented interface.
    Implements,
}

/// View over a member descriptor (0x1C bytes, 0x20 with a type reference).
///
/// See the [module documentation](self) for the layout.
#[derive(Clone, Copy, Debug)]
pub struct MemberDesc<'a> {
    bytes: &'a [u8],
}

impl<'a> MemberDesc<'a> {
    /// Size of a descriptor without the type reference at 0x1C.
    pub const MIN_SIZE: usize = 0x1C;

    /// Parses a member descriptor from the given byte slice.
    ///
    /// Pass at least 0x20 bytes so [`var_type`](Self::var_type) can read the
    /// type reference of a typed object.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x1C`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::MIN_SIZE {
            return Err(Error::TooShort {
                expected: Self::MIN_SIZE,
                actual: data.len(),
                context: "MemberDesc",
            });
        }
        Ok(Self { bytes: data })
    }

    /// VA of the member's null-terminated name, at offset 0x00.
    #[inline]
    pub fn name_va(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x00)
    }

    /// Reads the member's name through `map`.
    ///
    /// # Errors
    ///
    /// Returns an error if the name VA cannot be read or resolved.
    pub fn name<'b>(&self, map: &AddressMap<'b>) -> Result<&'b [u8], Error> {
        let off = map.va_to_offset(self.name_va()?)?;
        read_cstr(map.file(), off)
    }

    /// The DWORD at offset 0x08: 0xFFFFFFFF for an `Implements` and a
    /// `WithEvents` variable (`members` `Mixed.Shown`), 0 for every other
    /// variable.
    #[inline]
    pub fn sink_raw(&self) -> Result<u32, Error> {
        read_u32_le(self.bytes, 0x08)
    }

    /// The member's DISPID at offset 0x0C, `None` for an `Implements`.
    ///
    /// `0x40030000` plus the variable's index among the object's public
    /// variables, in the order of the member array (`members` `Mixed`:
    /// `Value` 0, `Note` 1, `Shown` 2).
    pub fn memid(&self) -> Result<Option<u32>, Error> {
        let memid = read_u32_le(self.bytes, 0x0C)?;
        Ok((memid != u32::MAX).then_some(memid))
    }

    /// The u16 flags at offset 0x10 (2 for every descriptor in
    /// `tests/fixtures`).
    #[inline]
    pub fn flags(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x10)
    }

    /// The vtable byte offset of the variable's first accessor (its `Get`),
    /// at offset 0x12; `None` for an `Implements`.
    ///
    /// The `Let` and `Set` accessors follow in the next slots (see
    /// [`MethodLink`](crate::project::MethodLink)).
    pub fn vtable_offset(&self) -> Result<Option<u16>, Error> {
        let offset = read_u16_le(self.bytes, 0x12)?;
        Ok((offset != u16::MAX).then_some(offset))
    }

    /// Byte offset of the variable in an instance of the object, at offset
    /// 0x14; `None` for an `Implements`.
    pub fn instance_offset(&self) -> Result<Option<u32>, Error> {
        let offset = read_u32_le(self.bytes, 0x14)?;
        Ok((offset != u32::MAX).then_some(offset))
    }

    /// The member's type, from the code at offset 0x18 and, for a typed
    /// object, record or interface, the descriptor VA at 0x1C.
    ///
    /// For a variable of a project class (0x13) and for an `Implements`
    /// (also 0x13), [`ArgType::descriptor_va`] is that class's `ObjectInfo` VA:
    /// `members` `Holder.Peer As Holder` gives `Holder`'s, `Two`'s
    /// `Implements IFirst` gives `IFirst`'s. For an interface of a type
    /// library, [`ArgType::interface`] reads its library and IID (`members`
    /// `Holder.Auto As New Collection`: VBA's `_Collection`).
    ///
    /// # Errors
    ///
    /// Returns an error if the type code cannot be read.
    pub fn var_type(&self) -> Result<ArgType, Error> {
        let code = u8::try_from(read_u32_le(self.bytes, 0x18)? & 0xFF).unwrap_or(0);
        let t = ArgType::new(code);
        Ok(if t.has_descriptor() {
            t.with_descriptor(self.bytes.get(0x1C..0x20))
        } else {
            t
        })
    }

    /// Returns the bytes the descriptor occupies: 0x20 when its type carries
    /// a descriptor VA, else [`MIN_SIZE`](Self::MIN_SIZE).
    ///
    /// # Errors
    ///
    /// Returns an error if the type code cannot be read.
    pub fn size(&self) -> Result<usize, Error> {
        Ok(if self.var_type()?.has_descriptor() {
            Self::MIN_SIZE.saturating_add(4)
        } else {
            Self::MIN_SIZE
        })
    }

    /// Classifies the member: an `Implements` has no DISPID, a `WithEvents`
    /// variable a non-zero [`sink_raw`](Self::sink_raw).
    ///
    /// # Errors
    ///
    /// Returns an error if the DISPID or the field at 0x08 cannot be read.
    pub fn kind(&self) -> Result<MemberKind, Error> {
        Ok(if self.memid()?.is_none() {
            MemberKind::Implements
        } else if self.sink_raw()? != 0 {
            MemberKind::WithEvents
        } else {
            MemberKind::Variable
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // `members` Holder.Peer As Holder (VA 0x0040309C).
    const PEER: [u8; 0x20] = [
        0xE4, 0x3E, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x0B, 0x00, 0x03,
        0x40, 0x02, 0x00, 0x7C, 0x00, 0x74, 0x00, 0x00, 0x00, 0x13, 0x00, 0x00, 0x00, 0xF4, 0x1E,
        0x40, 0x00,
    ];

    // `members` Two's `Implements IFirst` (VA 0x00402FDC).
    const IFIRST: [u8; 0x20] = [
        0x08, 0x3F, 0x40, 0x00, 0x00, 0x00, 0x00, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF,
        0xFF, 0x02, 0x00, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0xFF, 0x13, 0x00, 0x00, 0x00, 0x08, 0x16,
        0x40, 0x00,
    ];

    #[test]
    fn test_variable() {
        let m = MemberDesc::parse(&PEER).unwrap();
        assert_eq!(m.name_va().unwrap(), 0x00403EE4);
        assert_eq!(m.memid().unwrap(), Some(0x4003000B));
        assert_eq!(m.flags().unwrap(), 2);
        assert_eq!(m.vtable_offset().unwrap(), Some(0x7C));
        assert_eq!(m.instance_offset().unwrap(), Some(0x74));
        assert_eq!(m.var_type().unwrap().descriptor_va(), Some(0x00401EF4));
        assert_eq!(m.kind().unwrap(), MemberKind::Variable);
    }

    #[test]
    fn test_implements() {
        let m = MemberDesc::parse(&IFIRST).unwrap();
        assert_eq!(m.memid().unwrap(), None);
        assert_eq!(m.vtable_offset().unwrap(), None);
        assert_eq!(m.instance_offset().unwrap(), None);
        assert_eq!(m.var_type().unwrap().descriptor_va(), Some(0x00401608));
        assert_eq!(m.kind().unwrap(), MemberKind::Implements);
    }

    #[test]
    fn test_parse_too_short() {
        assert!(MemberDesc::parse(&[0u8; 0x1B]).is_err());
    }
}
