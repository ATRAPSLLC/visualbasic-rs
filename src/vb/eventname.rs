//! Event names for the slots of a control's event sink vtable.
//!
//! A control's [`EventSinkVtable`](crate::vb::events::EventSinkVtable) has
//! one slot per member of the control's events interface: slot `k` is the
//! member at vtable offset `0x0C + 4 * k` of that interface (the events
//! interfaces of `VB6.OLB` derive from `IUnknown`), so the order differs per
//! control class (`CommandButton`: `Click` is slot 0; `TextBox`: `Change`
//! is slot 0 and `Click` slot 15; `Form`: `Load` is slot 6, `Unload` slot 8).
//! A class module's own ControlInfo uses `IClassModuleEvt` (`Initialize`,
//! `Terminate`); a UserControl placed on a form has the 9 extender events
//! first (`Extender`), then its own events in declaration order.
//!
//! The tables are generated at build time from `data/vb6_events.csv`, the
//! events interfaces of `VB6.OLB` (and `IClassModuleEvt` of the runtime's
//! type library). Verified on
//! `tests/fixtures/forms` (Form `Load` 6, `Unload` 8; CommandButton `Click`
//! 0; UserControl `Resize` 7, `Paint` 22, `InitProperties` 33),
//! `tests/fixtures/controls` and `tests/fixtures/events` (`Class`).

use crate::vb::{control::generated, formdata::FormControlType};

/// Returns the event name for sink slot `slot` of a control of type `ctype`.
///
/// Returns `None` if the class has no event at the slot or no table
/// ([`FormControlType::Unknown`], a UserControl instance on its host: see
/// [`event_name_for_class`] with `"Extender"`).
pub fn event_name(slot: u16, ctype: FormControlType) -> Option<&'static str> {
    event_name_for_class(ctype.name(), slot)
}

/// Returns the event name for sink slot `slot` of control class `class`
/// (a name [`Guid::control_class_name`](crate::vb::control::Guid::control_class_name)
/// or [`FormControlType::name`] returns, `"Class"` for a class module's
/// `Initialize` / `Terminate`, `"Extender"` for the first 9 slots of a
/// UserControl instance).
pub fn event_name_for_class(class: &str, slot: u16) -> Option<&'static str> {
    generated::EVENTS
        .iter()
        .find(|(name, _)| *name == class)?
        .1
        .iter()
        .find(|(s, _)| *s == slot)
        .map(|(_, name)| *name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_command_button() {
        assert_eq!(event_name(0, FormControlType::CommandButton), Some("Click"));
        assert_eq!(
            event_name(5, FormControlType::CommandButton),
            Some("KeyPress")
        );
    }

    #[test]
    fn test_text_box() {
        assert_eq!(event_name(0, FormControlType::TextBox), Some("Change"));
        assert_eq!(event_name(15, FormControlType::TextBox), Some("Click"));
        assert_eq!(event_name(23, FormControlType::TextBox), Some("Validate"));
    }

    #[test]
    fn test_timer() {
        assert_eq!(event_name(0, FormControlType::Timer), Some("Timer"));
        assert_eq!(event_name(1, FormControlType::Timer), None);
    }

    #[test]
    fn test_form() {
        assert_eq!(event_name(6, FormControlType::Form), Some("Load"));
        assert_eq!(event_name(8, FormControlType::Form), Some("Unload"));
        assert_eq!(event_name(23, FormControlType::Form), Some("Initialize"));
    }

    #[test]
    fn test_usercontrol() {
        assert_eq!(event_name(7, FormControlType::UserControl), Some("Resize"));
        assert_eq!(event_name(22, FormControlType::UserControl), Some("Paint"));
        assert_eq!(
            event_name(33, FormControlType::UserControl),
            Some("InitProperties")
        );
        // A gap: _UserControl has no Link* events.
        assert_eq!(event_name(4, FormControlType::UserControl), None);
    }

    #[test]
    fn test_class_and_extender() {
        assert_eq!(event_name_for_class("Class", 0), Some("Initialize"));
        assert_eq!(event_name_for_class("Class", 1), Some("Terminate"));
        assert_eq!(event_name_for_class("Extender", 8), Some("Validate"));
        assert_eq!(event_name_for_class("Extender", 9), None);
    }

    #[test]
    fn test_out_of_range() {
        assert_eq!(event_name(24, FormControlType::Label), None);
        assert_eq!(event_name(0, FormControlType::Unknown(0xFF)), None);
    }
}
