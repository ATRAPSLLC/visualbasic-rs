//! Variable descriptor tables (PublicBytes and StaticBytes).
//!
//! The `PublicObjectDescriptor.public_bytes_va` field points to a variable
//! descriptor table, and `PublicObjectDescriptor.static_bytes_va` (when
//! non-zero) to one for the object's `Static` locals. The format is the
//! same for every object type, standard modules included:
//!
//! | Offset | Size | Field |
//! |--------|------|-------|
//! | 0x00 | 2 | Total byte size of the structure |
//! | 0x02 | 2 | Size of the variable data block (module data, class instance, static block) |
//! | 0x04 | 2 | Number of entries that need initialization (fixed-size arrays in the fixtures) |
//! | 0x06 | 2 | Number of entries |
//! | 0x08 | 4 | Unknown (0, except 0x10000000 in both tables of `tests/fixtures/statics` `Program`) |
//! | 0x0C | var | Entries, variable-length, in the [`controlprop`](super::controlprop) format |
//!
//! There is one entry per variable that needs initialization or cleanup
//! (`String`, `Variant`, `Object`, arrays, UDTs; Private as well as
//! Public), none for scalars such as `Long` (`statics` `Other` declares
//! `Public o_Value As Long` and `Public o_Text As String` and has one
//! entry, String at 0x04). An entry's type byte is a
//! [`ControlPropertyType`](super::controlprop::ControlPropertyType)
//! nibble: `0x01` is String (`flow`: `Private m_Log As String` at offset 0).
//! The table holds no variable names.
//!
//! The runtime walks all three kinds of table with one routine (MSVBVM60
//! 6.00.8176 `0x6600E62B`): it visits `+0x06` entries from +0x0C, sizing
//! each with `0x660399BA`, and stops once `+0x04` of them have been
//! initialized. For a module it first zero-fills the module data block with
//! the `+0x02` size (6.00.8176 `0x660276C1`). `EbLoadRunTime` stores each
//! object's `+0x02` size in its per-object record (6.00.9848
//! `0x6602F883`).
//!
//! [`ClassFormPublicBytes`] reads this format for every object type.

use crate::{error::Error, util::read_u16_le, vb::controlprop::ControlPropertyIter};

/// View over a variable descriptor table (see the [module docs](self)).
///
/// The table at `PublicObjectDescriptor.public_bytes_va` (or
/// `static_bytes_va`) of any object: standard module, class, form or
/// UserControl. [`VbObject::public_bytes`](crate::project::VbObject::public_bytes)
/// and [`VbObject::static_bytes`](crate::project::VbObject::static_bytes)
/// read it. Despite the type's name and its accessors' (`control_*`,
/// `property_count`), the entries describe variables.
///
/// # Runtime Access (MSVBVM60 6.00.9848 addresses)
///
/// - `EbLoadRunTime` (`0x6602F6CE`): reads `+0x02` (wInstanceSize) into its
///   per-object record at `+0x1C` (`0x6602F883`)
/// - `0x6602B56D`: zero-fills a module's data block with `+0x02` bytes
/// - `0x6601505E`: reads `+0x04` (wPropertyCount), `+0x06` (wControlCount),
///   then walks the entries from `+0x0C`
/// - no read of `+0x00` (wDataSize) was found
///
/// # Layout
///
/// | Offset | Size | Field | Runtime reads? |
/// |--------|------|-------|----------------|
/// | 0x00 | 2 | `wDataSize` - total size of the table | No |
/// | 0x02 | 2 | `wInstanceSize` - size of the variable data block | Yes |
/// | 0x04 | 2 | `wPropertyCount` - entries that need initialization | Yes |
/// | 0x06 | 2 | `wControlCount` - number of entries | Yes |
/// | 0x08 | 4 | Reserved / flags | No |
/// | 0x0C | var | Variable entries (typed, variable-length) | Yes (when counts > 0) |
///
/// The structure is `wDataSize` bytes long: 0x0C when both counts are 0
/// (every such class and form in `tests/fixtures`). What follows it belongs
/// to other structures (Board in `tests/fixtures/forms`: the `Global` class
/// record of another object's constant pool; Gauge: a BSTR), so nothing
/// past `wDataSize` is read. The object's IIDs are in its
/// [`OptionalObjectInfo`](super::object::OptionalObjectInfo).
#[derive(Clone, Copy, Debug)]
pub struct ClassFormPublicBytes<'a> {
    bytes: &'a [u8],
}

impl<'a> ClassFormPublicBytes<'a> {
    /// Minimum size to read the header fields.
    pub const MIN_SIZE: usize = 0x0C;

    /// Parses a variable descriptor table from the given byte slice.
    ///
    /// # Errors
    ///
    /// Returns [`Error::TooShort`] if `data.len() < 0x0C`.
    pub fn parse(data: &'a [u8]) -> Result<Self, Error> {
        if data.len() < Self::MIN_SIZE {
            return Err(Error::TooShort {
                expected: Self::MIN_SIZE,
                actual: data.len(),
                context: "ClassFormPublicBytes",
            });
        }
        Ok(Self { bytes: data })
    }

    /// Total size of the structure at offset 0x00: the 0x0C-byte header
    /// plus the entries.
    ///
    /// 0x0C when there are no entries (every form in the fixtures, and
    /// `calls` `Shape`, `events` `Source`); 0x10 for `flow`'s module
    /// (`m_Log As String`); 0x46 for `data`'s module; 0x64 for `dispid` `Bag`.
    #[inline]
    pub fn data_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x00)
    }

    /// Size of the variable data block at offset 0x02: the instance size of
    /// a class, form or UserControl, the data block of a module, the static
    /// block of a `Static` table.
    ///
    /// Read by `EbLoadRunTime` into its per-object record (6.00.9848
    /// `0x6602F883`); for a module the runtime zero-fills the block at
    /// [`PublicObjectDescriptor::module_public_va`](super::object::PublicObjectDescriptor::module_public_va)
    /// with this size (6.00.8176 `0x66027700`). Equal to
    /// [`PrivateObjectDescriptor::instance_size`](super::privateobj::PrivateObjectDescriptor::instance_size)
    /// for every fixture object that has one. Fixtures: classes 0x40 (no
    /// member variables: `calls` `Shape`, `events` `Measure`) to 0x88
    /// (`dispid` `Bag`); forms 0x44 to 0x54; UserControls 0x50 and 0x94;
    /// modules 0x08 (`flow`) to 0x44 (`statics` `Program`).
    #[inline]
    pub fn instance_size(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x02)
    }

    /// Number of entries that need initialization, at offset 0x04.
    ///
    /// The runtime's entry walk stops once this many entries have been
    /// initialized (MSVBVM60 6.00.8176 `0x6600E64F`). In the fixtures it
    /// counts the fixed-size arrays (`data` `Program` 1, `dispid` `Bag` 2).
    #[inline]
    pub fn property_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x04)
    }

    /// Number of entries at offset 0x06.
    ///
    /// The runtime's walk visits at most this many (6.00.8176
    /// `0x6600E68F`). Each entry at +0x0C is a typed, variable-length
    /// [`controlprop`](super::controlprop) entry. 0 for a table with only
    /// its header.
    #[inline]
    pub fn control_count(&self) -> Result<u16, Error> {
        read_u16_le(self.bytes, 0x06)
    }

    /// Returns `true` if the table has entries.
    #[inline]
    pub fn has_controls(&self) -> bool {
        self.control_count().is_ok_and(|c| c > 0)
    }

    /// Raw bytes of the entry array: from +0x0C to `wDataSize` (or the end
    /// of the backing slice, if shorter).
    ///
    /// Empty when the structure is only its 0x0C-byte header.
    pub fn entry_data(&self) -> &'a [u8] {
        let end = self.data_size().map_or(self.bytes.len(), |size| {
            usize::from(size).min(self.bytes.len())
        });
        self.bytes.get(0x0C..end).unwrap_or(&[])
    }

    /// Returns an iterator over the variable entries starting at +0x0C.
    ///
    /// See [`controlprop`](super::controlprop) for entry types and format.
    /// `tests/fixtures/flow`'s module yields one String entry at offset 0
    /// (`Private m_Log As String`); `statics` `Program` String at 0x0C, UDT
    /// at 0x10 and array at 0x24.
    pub fn control_entries(&self) -> ControlPropertyIter<'a> {
        ControlPropertyIter::new(self.entry_data(), self.control_count().unwrap_or(0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vb::controlprop::ControlPropertyType;

    // A module's table from an external sample (mod_Variaveis): 15 entries,
    // String entries at 0x00..0x30, a dynamic array at 0x3C, String at 0x40.
    const MOD_VARIAVEIS: [u8; 78] = [
        0x4E, 0x00, 0x48, 0x00, 0x00, 0x00, 0x0F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        0x00, 0x04, 0x00, 0x01, 0x00, 0x08, 0x00, 0x01, 0x00, 0x0C, 0x00, 0x01, 0x00, 0x10, 0x00,
        0x01, 0x00, 0x14, 0x00, 0x01, 0x00, 0x18, 0x00, 0x01, 0x00, 0x1C, 0x00, 0x01, 0x00, 0x20,
        0x00, 0x01, 0x00, 0x24, 0x00, 0x01, 0x00, 0x28, 0x00, 0x01, 0x00, 0x2C, 0x00, 0x01, 0x00,
        0x30, 0x00, 0x01, 0x00, 0x3C, 0x00, 0x05, 0x01, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x40,
        0x00, 0x01, 0x00,
    ];

    #[test]
    fn test_module_table_entries() {
        let table = ClassFormPublicBytes::parse(&MOD_VARIAVEIS).unwrap();
        assert_eq!(table.data_size().unwrap(), 0x4E);
        assert_eq!(table.instance_size().unwrap(), 0x48);
        assert_eq!(table.control_count().unwrap(), 15);
        let entries: Vec<_> = table.control_entries().take(14).collect();
        for (i, entry) in entries.iter().take(13).enumerate() {
            assert_eq!(usize::from(entry.frame_offset().unwrap()), 4 * i);
            assert_eq!(entry.property_type(), ControlPropertyType::String);
        }
        assert_eq!(entries[13].frame_offset().unwrap(), 0x3C);
        assert_eq!(entries[13].property_type(), ControlPropertyType::Array);
    }

    // Real data from Form1 in pe_x86_vb_loader sample (no embedded controls)
    const FORM1_PUBLIC_BYTES: [u8; 0x40] = [
        0x0C, 0x00, 0x44, 0x00, // +0x00: data_size=12, instance_size=68
        0x00, 0x00, 0x00, 0x00, // +0x04: property_count=0, control_count=0
        0x00, 0x00, 0x00, 0x00, // +0x08: reserved
        0x23, 0x3D, 0xFB, 0xFC, 0xFA, 0xA0, 0x68, 0x10, // +0x0C: default IID (no controls)
        0xA7, 0x38, 0x08, 0x00, 0x2B, 0x33, 0x71, 0xB5, 0x22, 0x3D, 0xFB, 0xFC, 0xFA, 0xA0, 0x68,
        0x10, // +0x1C: events IID (no controls)
        0xA7, 0x38, 0x08, 0x00, 0x2B, 0x33, 0x71, 0xB5, 0x02, 0x00, 0x00,
        0x00, // +0x2C: GUID pointer count
        0x68, 0x2F, 0x40, 0x00, // +0x30: VA to default IID
        0x78, 0x2F, 0x40, 0x00, // +0x34: VA to events IID
        0x00, 0x00, 0x00, 0x00, // +0x38: zero
        0x79, 0x4F, 0xAD, 0x33, // +0x3C: unknown
    ];

    // Real data from Cls_CRC32 in pe_x86_vb_loader sample (has controls)
    const CLS_CRC32_PUBLIC_BYTES: [u8; 0x18] = [
        0x38, 0x00, 0x60, 0x00, // +0x00: data_size=56, instance_size=96
        0x01, 0x00, 0x01, 0x00, // +0x04: property_count=1, control_count=1
        0x00, 0x00, 0x00, 0x00, // +0x08: reserved
        0x38, 0x00, 0x05, 0x00, // +0x0C: first control entry (type=5 at +0x0E)
        0x5C, 0x00, 0x55, 0x00, // +0x10: entry data continues...
        0x00, 0x00, 0x65, 0x00, // +0x14: ...
    ];

    #[test]
    fn test_form_no_controls() {
        let cfpb = ClassFormPublicBytes::parse(&FORM1_PUBLIC_BYTES).unwrap();
        assert_eq!(cfpb.data_size().unwrap(), 0x0C);
        assert_eq!(cfpb.instance_size().unwrap(), 0x44);
        assert_eq!(cfpb.property_count().unwrap(), 0);
        assert_eq!(cfpb.control_count().unwrap(), 0);
        assert!(!cfpb.has_controls());
        // The structure ends at wDataSize: what follows is not its data.
        assert!(cfpb.entry_data().is_empty());
    }

    #[test]
    fn test_class_with_controls() {
        let cfpb = ClassFormPublicBytes::parse(&CLS_CRC32_PUBLIC_BYTES).unwrap();
        assert_eq!(cfpb.data_size().unwrap(), 0x38);
        assert_eq!(cfpb.instance_size().unwrap(), 0x60);
        assert_eq!(cfpb.property_count().unwrap(), 1);
        assert_eq!(cfpb.control_count().unwrap(), 1);
        assert!(cfpb.has_controls());
        // Entry data is available
        assert!(!cfpb.entry_data().is_empty());
    }

    #[test]
    fn test_class_control_entries() {
        let cfpb = ClassFormPublicBytes::parse(&CLS_CRC32_PUBLIC_BYTES).unwrap();
        let entries: Vec<_> = cfpb.control_entries().collect();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].frame_offset().unwrap(), 0x38);
        assert_eq!(entries[0].property_type(), ControlPropertyType::Array);
        assert_eq!(entries[0].flags(), 0x00);
    }

    #[test]
    fn test_form_no_control_entries() {
        let cfpb = ClassFormPublicBytes::parse(&FORM1_PUBLIC_BYTES).unwrap();
        assert_eq!(cfpb.control_entries().count(), 0);
    }

    #[test]
    fn test_class_form_too_short() {
        assert!(ClassFormPublicBytes::parse(&[0; 0x0B]).is_err());
    }
}
