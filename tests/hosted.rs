//! A hosted control's record (an ActiveX control, a project's UserControl:
//! type 0xFF) states its extender's properties after its ProgID: its place,
//! index and tab index, around the object it persisted. Every fixture's
//! source states them too, so the source is the oracle.

#![allow(
    clippy::arithmetic_side_effects,
    clippy::expect_used,
    clippy::indexing_slicing,
    clippy::panic,
    clippy::unwrap_used
)]

mod common;

use std::{collections::BTreeMap, fs, path::Path};

use common::{fixture, projects};
use visualbasic::{
    VbObject, VbProject,
    vb::{
        formdata::{FormControlType, FormDataParser},
        property::PropertyValue,
    },
};

/// The project of fixture `name`, alive for the test.
fn project(name: &str) -> &'static VbProject<'static> {
    let bytes: &'static [u8] = Box::leak(fs::read(fixture(name)).unwrap().into_boxed_slice());
    Box::leak(Box::new(VbProject::from_bytes(bytes).unwrap()))
}

/// The object named `name`.
fn object(project: &'static VbProject<'static>, name: &str) -> VbObject<'static, 'static> {
    project
        .objects()
        .unwrap()
        .map(Result::unwrap)
        .find(|o| o.name().unwrap() == name)
        .unwrap_or_else(|| panic!("no object {name}"))
}

/// The form data of the designer `name`.
fn form_data(project: &'static VbProject<'static>, name: &str) -> FormDataParser<'static> {
    let designer = object(project, name);
    project
        .gui_entries()
        .unwrap()
        .find_map(|entry| {
            designer
                .form_data_from_gui_entry(&entry)
                .filter(|data| data.form_name() == name)
        })
        .unwrap_or_else(|| panic!("{name}: no form data"))
}

/// What a source file states of its hosted controls: the designer's name,
/// then per control (by name and index) its `Left`, `Top`, `Width`,
/// `Height` and `TabIndex`.
type Stated = (
    String,
    BTreeMap<(String, Option<i64>), BTreeMap<String, i64>>,
);

/// Reads the hosted controls of the designer source `text`: each
/// `Begin <Library>.<Class> <Name>` block whose library is not `VB`, its
/// own lines only (`BeginProperty` blocks and nested controls skipped).
fn stated(text: &str) -> Option<Stated> {
    let mut designer = None;
    let mut open: Vec<Option<(String, BTreeMap<String, i64>)>> = Vec::new();
    let mut in_property = 0;
    let mut controls = BTreeMap::new();
    for line in text.lines().map(str::trim) {
        if line.starts_with("BeginProperty") {
            in_property += 1;
        } else if line.starts_with("EndProperty") {
            in_property -= 1;
        } else if let Some(rest) = line.strip_prefix("Begin ") {
            let mut words = rest.split_whitespace();
            let class = words.next().unwrap();
            let name = words.next().unwrap().to_string();
            if designer.is_none() {
                designer = Some(name);
                open.push(None);
            } else if class.starts_with("VB.") {
                open.push(None);
            } else {
                open.push(Some((name, BTreeMap::new())));
            }
        } else if line == "End" {
            if let Some(Some((name, values))) = open.pop() {
                let index = values.get("Index").copied();
                controls.insert((name, index), values);
            }
        } else if in_property == 0
            && let Some(Some((_, values))) = open.last_mut()
            && let Some((key, value)) = line.split_once('=')
            && let Ok(number) = value.split('\'').next().unwrap().trim().parse::<i64>()
        {
            values.insert(key.trim().to_string(), number);
        }
    }
    Some((designer?, controls))
}

/// Every hosted control of every fixture decodes its extender's properties
/// to the stream's terminator, and its place, index and tab index are the
/// ones its source states, except where the designer writes another value: `Bounds` for a control drawn at run time
/// (`forms` Gauge1: 120, 2280, 1500, 600), `Left` and `Top` alone for one
/// that is not (`activex` CommonDialog1: 1680, 120), after its persisted
/// object.
#[test]
fn a_hosted_controls_place_is_its_sources() {
    let mut checked = 0;
    let mut mismatches = Vec::new();
    for name in projects() {
        let dir = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(&name);
        let mut sources: Vec<_> = fs::read_dir(&dir)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|e| e.path()))
            .filter(|path| {
                matches!(
                    path.extension().and_then(|e| e.to_str()),
                    Some("frm" | "ctl" | "pag" | "dob")
                )
            })
            .collect();
        sources.sort();
        for source in sources {
            let Some((designer, controls)) = stated(&fs::read_to_string(&source).unwrap()) else {
                continue;
            };
            if controls.is_empty() {
                continue;
            }
            let data = form_data(project(&name), &designer);
            for ((control, index), values) in &controls {
                let record = data
                    .controls()
                    .iter()
                    .find(|r| {
                        r.name() == control.as_str() && r.array_index().map(i64::from) == *index
                    })
                    .unwrap_or_else(|| panic!("{name}/{designer}: no record {control}"));
                assert_eq!(record.control_type(), FormControlType::Unknown(0xFF));
                let mut iter = record.properties();
                let mut decoded = BTreeMap::new();
                for property in iter.by_ref() {
                    match property.value {
                        PropertyValue::Bounds(b) => {
                            decoded.insert("Left", i64::from(b.left));
                            decoded.insert("Top", i64::from(b.top));
                            decoded.insert("Width", i64::from(b.width));
                            decoded.insert("Height", i64::from(b.height));
                        }
                        PropertyValue::Long(v) => {
                            decoded.insert(property.name, i64::from(v));
                        }
                        PropertyValue::Int16(v) => {
                            decoded.insert(property.name, i64::from(v));
                        }
                        _ => {}
                    }
                }
                // The stream ends at its terminator, which the record's raw
                // bytes leave out.
                assert!(
                    iter.at_terminator() || iter.position() == record.raw_properties().len(),
                    "{name}/{designer}/{control}: decoding stopped at {} of {}",
                    iter.position(),
                    record.raw_properties().len()
                );
                for key in ["Left", "Top", "Width", "Height", "Index", "TabIndex"] {
                    if decoded.get(key) != values.get(key) {
                        mismatches.push(format!(
                            "{name}/{designer}/{control}.{key}: {:?} stated {:?}",
                            decoded.get(key),
                            values.get(key)
                        ));
                    }
                }
                checked += 1;
            }
        }
    }
    // Where the compiled record holds another value than the source, as the
    // designer writes it (`extender` Board): a control invisible at run time
    // (Ghost1) stores its place without a size, and neither it nor one that
    // cannot take the focus (NoFocus1) has a tab index; the tab slots they
    // leave (2 and 4) are given to the later Light1 and Coords1.
    assert_eq!(
        mismatches,
        [
            "extender/Board/Coords1.TabIndex: Some(4) stated Some(8)",
            "extender/Board/Ghost1.Width: None stated Some(1500)",
            "extender/Board/Ghost1.Height: None stated Some(600)",
            "extender/Board/Ghost1.TabIndex: None stated Some(4)",
            "extender/Board/Light1.TabIndex: Some(2) stated Some(9)",
            "extender/Board/NoFocus1.TabIndex: None stated Some(2)",
        ]
    );
    assert!(checked >= 10, "only {checked} hosted controls checked");
}

/// The header's words at 0x59 and 0x5D are lengths, not the form's size:
/// the form's stream runs from 0x5D for the first, and its first child
/// record starts the second's bytes after 0x5D (`forms` Board 483 and 87,
/// its client size 4680 x 3090 twips in its `ClientSize`).
#[test]
fn the_header_states_the_streams_lengths() {
    let data = form_data(project("forms"), "Board");
    assert_eq!(data.header().stream_size().unwrap(), 483);
    assert_eq!(data.header().properties_size().unwrap(), 87);
    let client = data
        .form_properties_decoded()
        .find_map(|p| match p.value {
            PropertyValue::ClientRect(r) => Some((r.width, r.height)),
            _ => None,
        })
        .unwrap();
    assert_eq!(client, (4680, 3090));
    let first = data.controls().first().unwrap();
    // The record starts with its marker byte there; its size field follows.
    assert_eq!(first.offset_in_blob(), 0x5D + 87 + 1);
}
