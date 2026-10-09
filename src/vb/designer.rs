//! The designers a VB6 object can be built with: forms, UserControls and
//! the other ActiveX designers of `VB6.OLB`.
//!
//! A designer object names its designer in its own ControlInfo (control
//! index 0xFFFF), whose GUID is the designer's **events** interface; its
//! vtable starts with the designer's **built-in** interface (`_Form`,
//! `_UserControl`, ...), then 256 control getters, then the object's own
//! members (see [`CallResolver`](crate::pcode::calltarget::CallResolver)).
//! The GUIDs and vtable sizes are `VB6.OLB`'s, and every designer's are
//! measured: Form and UserControl on `tests/fixtures/{forms,controls,dispid}`,
//! MDIForm on `mdi`, UserDocument on `docs`, PropertyPage on `ocx` (the
//! events IID in the object's own ControlInfo, the built-in size in its
//! inherited vtable slots, `(size + 0x400 - 0x1C) / 4`).

use crate::vb::control::Guid;

/// A designer of `VB6.OLB`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Designer {
    /// A form (`_Form`, `FormEvents`).
    Form,
    /// An MDI form (`_MDIForm`, `MDIFormEvents`).
    MdiForm,
    /// A UserControl (`_UserControl`, `UserControlEvents`).
    UserControl,
    /// A UserDocument (`_UserDocument`, `UserDocumentEvents`).
    UserDocument,
    /// A PropertyPage (`_PropertyPage`, `PropertyPageEvents`).
    PropertyPage,
}

impl Designer {
    /// Every designer.
    pub const ALL: [Self; 5] = [
        Self::Form,
        Self::MdiForm,
        Self::UserControl,
        Self::UserDocument,
        Self::PropertyPage,
    ];

    /// The bytes after `Data1` that every `VB6.OLB` GUID here shares:
    /// `-6699-11CF-B70C-00AA0060D393`.
    const TAIL: [u8; 12] = [
        0x99, 0x66, 0xCF, 0x11, 0xB7, 0x0C, 0x00, 0xAA, 0x00, 0x60, 0xD3, 0x93,
    ];

    /// The `Data1` of the events interface and of the built-in interface,
    /// and the built-in interface's vtable size (`cbSizeVft`).
    const fn facts(self) -> (u32, u32, u16) {
        match self {
            Self::Form => (0x33AD_4F3A, 0x33AD_4F39, 0x2F8),
            Self::MdiForm => (0x33AD_4F72, 0x33AD_4F71, 0x2F8),
            Self::UserControl => (0x33AD_5012, 0x33AD_5011, 0x3A4),
            Self::UserDocument => (0x33AD_5022, 0x33AD_5021, 0x370),
            Self::PropertyPage => (0x33AD_501A, 0x33AD_5019, 0x310),
        }
    }

    /// The `VB6.OLB` GUID with this `Data1`.
    fn guid(data1: u32) -> Guid {
        let mut bytes = [0u8; 16];
        let (head, tail) = bytes.split_at_mut(4);
        head.copy_from_slice(&data1.to_le_bytes());
        tail.copy_from_slice(&Self::TAIL);
        Guid { bytes }
    }

    /// Returns the designer whose events interface `guid` is: the GUID a
    /// designer object's own ControlInfo names.
    pub fn from_events_iid(guid: &Guid) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|designer| designer.events_iid() == *guid)
    }

    /// Returns the designer's events interface IID.
    pub fn events_iid(self) -> Guid {
        Self::guid(self.facts().0)
    }

    /// Returns the IID of the designer's built-in interface, the start of
    /// every one of its objects' vtables.
    pub fn interface_iid(self) -> Guid {
        Self::guid(self.facts().1)
    }

    /// Returns the size in bytes of the built-in interface's vtable: the
    /// offset of the first control getter.
    pub fn builtin_vtable_size(self) -> u16 {
        self.facts().2
    }

    /// Returns the designer's name: `"Form"`, `"MDIForm"`, `"UserControl"`,
    /// `"UserDocument"`, `"PropertyPage"`.
    pub fn name(self) -> &'static str {
        match self {
            Self::Form => "Form",
            Self::MdiForm => "MDIForm",
            Self::UserControl => "UserControl",
            Self::UserDocument => "UserDocument",
            Self::PropertyPage => "PropertyPage",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_round_trip() {
        for designer in Designer::ALL {
            assert_eq!(
                Designer::from_events_iid(&designer.events_iid()),
                Some(designer)
            );
            assert_ne!(designer.events_iid(), designer.interface_iid());
        }
        assert_eq!(
            Designer::Form.events_iid().to_string(),
            "{33AD4F3A-6699-11CF-B70C-00AA0060D393}"
        );
        assert_eq!(Designer::UserControl.builtin_vtable_size(), 0x3A4);
    }
}
