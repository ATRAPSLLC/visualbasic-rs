//! Frame variable resolution for P-Code stack offsets.
//!
//! Resolves `%a` operands (`StackVar(i16)` EBP-relative offsets) to
//! named variables with type information. Every P-Code procedure has the
//! same frame shape (see [`pcode_frame`]):
//!
//! - **Arguments** from `ebp+0x0C`, after the `ebp+0x08` slot that holds `Me`
//!   in an object's method and the module's data block in a standard
//!   module's procedure.
//! - **Housekeeping** from `ebp-0x04` to `ebp-0x84`: the interpreter's slots.
//! - **Locals** below `ebp-0x84`, `ProcDscInfo.wFrameSize` bytes.

use std::collections::BTreeMap;

use crate::{
    addressmap::AddressMap,
    pcode::decoder::{Instruction, ProcedureReturn},
    project::PCodeMethod,
    vb::{
        controlprop::ControlPropertyType,
        functype::{ArgType, FuncTypDesc},
        object::ObjectInfo,
        procedure::{ProcDscInfo, pcode_frame},
    },
};

/// Resolved information about a frame variable.
#[derive(Debug, Clone)]
pub enum FrameVar {
    /// Local variable within the procedure's frame.
    Local {
        /// Byte offset from the start of the local variable area: `ebp-0x88`
        /// is offset 4.
        frame_offset: u16,
    },
    /// Function argument.
    Argument {
        /// Zero-based parameter index when the procedure's [`FuncTypDesc`] is
        /// known; otherwise the index of the 4-byte slot from `ebp+0x0C` (an
        /// 8-byte or Variant argument takes several).
        index: u8,
        /// Parameter name from the FuncTypDesc, if available.
        name: Option<String>,
        /// Parameter type from the FuncTypDesc, if available.
        arg_type: Option<ArgType>,
    },
    /// The pointer to the caller's return-value slot: the last argument of a
    /// public method with a return value (its `[out, retval]` parameter).
    ReturnValue {
        /// The return type, with the ByRef bit of the pointer.
        arg_type: ArgType,
    },
    /// The hidden first argument (`ebp+0x0C`) of a standard module's
    /// function returning a Variant or a record: the pointer its
    /// `ExitProcCb` / `ExitProcFrameCb` copies the result to (see
    /// [`FrameResolver::for_method`]).
    HiddenResult,
    /// Runtime housekeeping slot (pcode_ip, const_pool_va, etc.).
    Housekeeping {
        /// Named constant from the pcode_frame module.
        name: &'static str,
    },
    /// Offset that doesn't fit recognized patterns.
    Unknown {
        /// The raw EBP offset.
        offset: i16,
    },
}

/// One frame slot a procedure uses: an argument, a local, a runtime slot.
///
/// Returned by [`FrameResolver::declarations`].
#[derive(Debug, Clone)]
pub struct FrameDeclaration {
    /// The slot's offset from ebp.
    pub offset: i16,
    /// What the slot is.
    pub var: FrameVar,
    /// The type the procedure's cleanup table gives the slot, for a local
    /// the runtime releases on exit (a String, Variant, object, array or
    /// record).
    pub cleanup: Option<ControlPropertyType>,
}

/// What a procedure's `ebp+8` slot holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FrameOwner {
    /// A method of a class, form or control: `ebp+8` is `Me`.
    Object,
    /// A standard module's procedure: `ebp+8` is the module's data block,
    /// which `ProcCallEngine` inserts.
    Module,
    /// The procedure's ObjectInfo could not be read.
    Unknown,
}

/// Resolves EBP-relative stack offsets to named frame variables.
///
/// Constructed once per method from `ProcDscInfo` and optional
/// `FuncTypDesc`, then reused for every `%a` operand in that method.
pub struct FrameResolver {
    frame_size: u16,
    /// What `ebp+8` holds.
    owner: FrameOwner,
    param_names: Vec<String>,
    arg_types: Vec<ArgType>,
    return_type: Option<ArgType>,
    /// Each parameter's offset from ebp, from the FuncTypDesc's widths, then
    /// the return value pointer's.
    param_offsets: Vec<i16>,
    /// The cleanup table's types, by frame offset.
    cleanup: BTreeMap<i16, ControlPropertyType>,
    /// `true` if `ebp+0x0C` holds a hidden result pointer.
    hidden_result: bool,
}

impl FrameResolver {
    /// Creates a resolver for the given procedure.
    ///
    /// The procedure's owner (object or standard module) comes from its
    /// ObjectInfo: a module's has no private object descriptor
    /// (`lpPrivateObject == 0xFFFFFFFF`). If `func_type` is available (a
    /// public method's prototype, from the `PrivateObjectDescriptor`), the
    /// parameters' names, types and offsets come from it, each as wide as its
    /// type, and the return value's pointer follows them. Otherwise
    /// arguments are numbered in 4-byte slots from `ebp+0x0C`.
    pub fn new(
        proc_dsc: &ProcDscInfo<'_>,
        func_type: Option<&FuncTypDesc<'_>>,
        map: &AddressMap<'_>,
    ) -> Self {
        let owner = proc_dsc
            .object_info_va()
            .and_then(|va| map.slice_from_va(va, ObjectInfo::SIZE))
            .and_then(ObjectInfo::parse)
            .and_then(|info| info.private_object_va())
            .map_or(FrameOwner::Unknown, |va| match va {
                u32::MAX => FrameOwner::Module,
                _ => FrameOwner::Object,
            });
        let (param_names, arg_types, return_type, param_offsets) = match func_type {
            Some(ftd) => (
                ftd.param_names(map)
                    .into_iter()
                    .map(|s| String::from_utf8_lossy(s).into_owned())
                    .collect(),
                ftd.arg_types(),
                ftd.return_type(),
                ftd.param_offsets(),
            ),
            None => (Vec::new(), Vec::new(), None, Vec::new()),
        };

        let cleanup = proc_dsc
            .cleanup_entries()
            .filter_map(|entry| {
                let offset = entry.frame_offset().ok()?.cast_signed();
                Some((offset, entry.property_type()))
            })
            .collect();

        Self {
            frame_size: proc_dsc.frame_size().unwrap_or(0),
            owner,
            param_names,
            arg_types,
            return_type,
            param_offsets,
            cleanup,
            hidden_result: false,
        }
    }

    /// Creates a resolver for a P-Code method, like [`new`](Self::new), and
    /// reads its code for a hidden result pointer: a procedure that returns
    /// through `ExitProcCb` / `ExitProcFrameCb` (a module function returning
    /// a Variant or a record) has the pointer at `ebp+0x0C`
    /// ([`FrameVar::HiddenResult`]) and its arguments from `ebp+0x10`
    /// (`tests/fixtures/types`: `MV`'s argument is `arg_10`).
    pub fn for_method(
        method: &PCodeMethod<'_>,
        func_type: Option<&FuncTypDesc<'_>>,
        map: &AddressMap<'_>,
    ) -> Self {
        let mut resolver = Self::new(method.proc_dsc(), func_type, map);
        resolver.hidden_result = method.instructions().is_ok_and(|mut code| {
            code.any(|insn| {
                insn.is_ok_and(|insn| {
                    matches!(
                        insn.procedure_return(),
                        Some(ProcedureReturn::CopyToHidden { .. })
                    )
                })
            })
        });
        resolver
    }

    /// Returns every frame slot `instructions` name - their `%a` operands
    /// and the slots `FFree*` payloads list
    /// ([`Instruction::frame_slots`]; `code` is the procedure's P-Code) -
    /// and every slot the cleanup table lists, by offset, resolved.
    pub fn declarations<'i>(
        &self,
        instructions: impl IntoIterator<Item = &'i Instruction>,
        code: &[u8],
    ) -> Vec<FrameDeclaration> {
        let mut offsets: Vec<i16> = instructions
            .into_iter()
            .flat_map(|insn| insn.frame_slots(code))
            .chain(self.cleanup.keys().copied())
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        offsets
            .into_iter()
            .map(|offset| FrameDeclaration {
                offset,
                var: self.resolve(offset),
                cleanup: self.cleanup.get(&offset).copied(),
            })
            .collect()
    }

    /// Returns what the procedure's `ebp+8` slot holds.
    pub fn owner(&self) -> FrameOwner {
        self.owner
    }

    /// Resolves an EBP-relative offset to a [`FrameVar`].
    ///
    /// # Layout
    ///
    /// ```text
    /// EBP+0x0C+..  = the arguments (then a public function's return pointer)
    /// EBP+0x08     = Me (object method) or the module's data block
    /// EBP+0x04     = saved return address
    /// EBP+0x00     = saved EBP
    /// EBP-0x04     = first housekeeping slot
    /// ...
    /// EBP-0x84     = last housekeeping slot (saved edi)
    /// EBP-0x88     = first local dword (a function's return value)
    /// ...
    /// EBP-(0x84+frame_size) = last local byte
    /// ```
    pub fn resolve(&self, offset: i16) -> FrameVar {
        let wide = i32::from(offset);
        if wide == pcode_frame::ME {
            return FrameVar::Housekeeping {
                name: match self.owner {
                    FrameOwner::Object => "me",
                    FrameOwner::Module => "module_data",
                    FrameOwner::Unknown => "this",
                },
            };
        }
        if wide >= pcode_frame::FIRST_ARG {
            if self.hidden_result && wide == pcode_frame::FIRST_ARG {
                return FrameVar::HiddenResult;
            }
            if self.param_offsets.is_empty() {
                let first = match self.hidden_result {
                    true => pcode_frame::FIRST_ARG.saturating_add(4),
                    false => pcode_frame::FIRST_ARG,
                };
                let slot = wide.saturating_sub(first) / 4;
                return FrameVar::Argument {
                    index: u8::try_from(slot).unwrap_or(u8::MAX),
                    name: None,
                    arg_type: None,
                };
            }
            // A parameter: the last one starting at or below `offset`.
            let Some(index) = self.param_offsets.iter().rposition(|&o| o <= offset) else {
                return FrameVar::Unknown { offset };
            };
            if index == self.arg_types.len()
                && let Some(arg_type) = self.return_type
            {
                return FrameVar::ReturnValue { arg_type };
            }
            return FrameVar::Argument {
                index: u8::try_from(index).unwrap_or(u8::MAX),
                name: self.param_names.get(index).cloned(),
                arg_type: self.arg_types.get(index).copied(),
            };
        }

        if offset >= 0 {
            // Saved EBP / return address area
            return FrameVar::Unknown { offset };
        }

        // Negative offset: check housekeeping vs local. Compute abs via i32 to
        // avoid `-i16::MIN` overflow in plain `-(offset as i32)`.
        let abs_offset = wide.unsigned_abs();

        if abs_offset <= pcode_frame::HOUSEKEEPING_SIZE {
            let name = match wide {
                pcode_frame::STATEMENT_IP => "statement_ip",
                pcode_frame::STATEMENT_ESP => "statement_esp",
                pcode_frame::GOSUB_DEPTH => "gosub_depth",
                pcode_frame::ERROR_STATEMENT => "error_statement",
                pcode_frame::ERROR_HANDLER => "error_handler",
                pcode_frame::ENGINE_CONTEXT => "engine_context",
                pcode_frame::FRAME_FLAGS => "frame_flags",
                pcode_frame::OBJECT_REGISTER => "object_register",
                pcode_frame::PROC_DSC_INFO => "proc_dsc_info",
                pcode_frame::CONST_POOL => "const_pool",
                pcode_frame::CODE_BASE => "code_base",
                _ => "housekeeping",
            };
            return FrameVar::Housekeeping { name };
        }

        // Local variable
        let local_byte = abs_offset.saturating_sub(pcode_frame::HOUSEKEEPING_SIZE);
        let frame_offset = u16::try_from(local_byte).unwrap_or(u16::MAX);
        if frame_offset <= self.frame_size {
            return FrameVar::Local { frame_offset };
        }

        FrameVar::Unknown { offset }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::addressmap::SectionEntry;

    /// One section at VA 0x401000 (file 0x200), 0x2000 bytes.
    fn make_test_map(file: &[u8]) -> AddressMap<'_> {
        AddressMap::from_parts(
            file,
            0x00400000,
            vec![SectionEntry {
                virtual_address: 0x1000,
                virtual_size: 0x2000,
                raw_data_offset: 0x200,
                raw_data_size: 0x2000,
            }],
        )
    }

    fn make_proc_dsc(object_info_va: u32, frame_size: u16, arg_size: u16) -> Vec<u8> {
        let mut data = vec![0u8; 0x1E]; // ProcDscInfo::MIN_SIZE
        data[0x00..0x04].copy_from_slice(&object_info_va.to_le_bytes());
        data[0x04..0x06].copy_from_slice(&arg_size.to_le_bytes());
        data[0x06..0x08].copy_from_slice(&frame_size.to_le_bytes());
        data[0x08..0x0A].copy_from_slice(&0x0050u16.to_le_bytes()); // proc_size
        data[0x0A..0x0C].copy_from_slice(&0x001Eu16.to_le_bytes()); // total_size
        data
    }

    /// A file whose ObjectInfo at VA 0x401000 has `private_object` at +0x0C.
    fn file_with_object_info(private_object: u32) -> Vec<u8> {
        let mut file = vec![0u8; 0x3000];
        file[0x20C..0x210].copy_from_slice(&private_object.to_le_bytes());
        file
    }

    #[test]
    fn test_module_arguments_start_at_0c() {
        let file = file_with_object_info(u32::MAX);
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0x401000, 0x100, 0x10);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, None, &map);
        assert_eq!(resolver.owner(), FrameOwner::Module);

        assert!(matches!(
            resolver.resolve(0x08),
            FrameVar::Housekeeping {
                name: "module_data"
            }
        ));
        assert!(
            matches!(
                resolver.resolve(0x0C),
                FrameVar::Argument {
                    index: 0,
                    name: None,
                    ..
                }
            ),
            "expected Argument(0), got {:?}",
            resolver.resolve(0x0C)
        );
        assert!(matches!(
            resolver.resolve(0x10),
            FrameVar::Argument { index: 1, .. }
        ));
    }

    #[test]
    fn test_object_method_has_me() {
        let file = file_with_object_info(0x401100);
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0x401000, 0x100, 0x08);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, None, &map);
        assert_eq!(resolver.owner(), FrameOwner::Object);
        assert!(matches!(
            resolver.resolve(0x08),
            FrameVar::Housekeeping { name: "me" }
        ));
        assert!(matches!(
            resolver.resolve(0x0C),
            FrameVar::Argument { index: 0, .. }
        ));
    }

    #[test]
    fn test_unreadable_object_info() {
        let file = vec![0u8; 0x3000];
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0, 0x100, 0x10);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, None, &map);
        assert_eq!(resolver.owner(), FrameOwner::Unknown);
        assert!(matches!(
            resolver.resolve(0x08),
            FrameVar::Housekeeping { name: "this" }
        ));
    }

    #[test]
    fn test_parameters_from_func_type() {
        // `Function F(ByVal a As Double, ByVal b As Long) As Integer`: entry
        // byte 3 << 2, has a return value; type list this, R8, I4, ByRef I2.
        let mut ftd_bytes = vec![0u8; 0x24];
        ftd_bytes[0x00] = 3 << 2;
        ftd_bytes[0x01] = 0x01;
        ftd_bytes[0x20..0x24].copy_from_slice(&[0x1E, 0x0B, 0x08, 0x26]);
        let ftd = FuncTypDesc::parse(&ftd_bytes).unwrap();
        let file = file_with_object_info(0x401100);
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0x401000, 0x10, 0x14);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, Some(&ftd), &map);

        // a: ebp+0x0C, 8 bytes wide.
        for offset in [0x0C, 0x10] {
            assert!(matches!(
                resolver.resolve(offset),
                FrameVar::Argument { index: 0, arg_type: Some(t), .. } if t.code() == 0x0B
            ));
        }
        // b: ebp+0x14.
        assert!(matches!(
            resolver.resolve(0x14),
            FrameVar::Argument { index: 1, arg_type: Some(t), .. } if t.code() == 0x08
        ));
        // The return value's pointer: ebp+0x18.
        assert!(matches!(
            resolver.resolve(0x18),
            FrameVar::ReturnValue { arg_type } if arg_type.code() == 0x26
        ));
    }

    #[test]
    fn test_resolve_housekeeping() {
        let file = vec![0u8; 0x3000];
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0, 0x100, 0x10);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, None, &map);

        // EBP-0x4C = Pr, the object register
        assert!(
            matches!(
                resolver.resolve(-0x4C),
                FrameVar::Housekeeping {
                    name: "object_register"
                }
            ),
            "expected Housekeeping(object_register), got {:?}",
            resolver.resolve(-0x4C)
        );

        // EBP-0x54 = the constant pool base
        assert!(
            matches!(
                resolver.resolve(-0x54),
                FrameVar::Housekeeping { name: "const_pool" }
            ),
            "expected Housekeeping(const_pool), got {:?}",
            resolver.resolve(-0x54)
        );
    }

    #[test]
    fn test_resolve_local() {
        let file = vec![0u8; 0x3000];
        let map = make_test_map(&file);
        let pdi_data = make_proc_dsc(0, 0x100, 0x10);
        let pdi = ProcDscInfo::parse(&pdi_data).unwrap();
        let resolver = FrameResolver::new(&pdi, None, &map);

        // EBP-0x88 = first local dword (a function's return value)
        assert!(
            matches!(resolver.resolve(-0x88), FrameVar::Local { frame_offset: 4 }),
            "expected Local(4), got {:?}",
            resolver.resolve(-0x88)
        );

        // EBP-0x8C = local at offset 8
        assert!(
            matches!(resolver.resolve(-0x8C), FrameVar::Local { frame_offset: 8 }),
            "expected Local(8), got {:?}",
            resolver.resolve(-0x8C)
        );

        // Beyond the frame.
        assert!(matches!(
            resolver.resolve(-0x200),
            FrameVar::Unknown { offset: -0x200 }
        ));
    }
}
