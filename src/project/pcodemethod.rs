//! P-Code method/procedure representation.
//!
//! A [`PCodeMethod`] provides access to a single P-Code procedure's metadata
//! (frame size, procedure size, flags) and a streaming iterator over its
//! decoded bytecode instructions.

use crate::{
    addressmap::AddressMap,
    error::Error,
    pcode::decoder::{ErrorFlow, InstructionIterator},
    util::read_u32_le,
    vb::{
        constantpool::ConstantPool,
        controlprop::ControlPropertyIter,
        procedure::{self, ProcDscInfo},
    },
};

/// A single P-Code method/procedure within a VB6 object.
///
/// Provides access to the procedure's metadata (frame size, proc size)
/// and a streaming iterator over its decoded P-Code instructions.
#[derive(Debug)]
pub struct PCodeMethod<'a> {
    /// Procedure descriptor (RTMI) containing frame size, proc size, and flags.
    proc_dsc: ProcDscInfo<'a>,
    /// Raw P-Code byte stream (slice into the file buffer).
    pcode_bytes: &'a [u8],
    /// Base VA of the constant pool, used to resolve string/API references.
    data_const_va: u32,
    /// VA of the first P-Code byte (= proc_dsc_va - proc_size).
    pcode_va: u32,
    /// VA of the ProcDscInfo (RTMI) structure.
    proc_dsc_va: u32,
    /// VA of the call stub (`mov edx, <RTMI>`) or direct ProcDscInfo pointer.
    stub_va: u32,
}

/// A beginning-of-statement marker discovered in a procedure's P-Code.
///
/// Returned by [`PCodeMethod::statement_markers`]. Each corresponds to a
/// `LargeBos` instruction the compiler emits at the start of a source
/// statement, only in a procedure that needs statement boundaries at run
/// time. In `tests/fixtures/flow` those are the procedures with `Resume`,
/// `Resume Next`, `On Error Resume Next` or line numbers; procedures whose
/// handler only exits or uses `Resume <label>`, and every procedure without
/// error handling, have none.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatementMarker {
    /// P-Code offset of the `LargeBos` marker (the statement boundary).
    pub offset: u16,
    /// Byte distance from this marker to the next statement boundary, taken
    /// from the marker's operand. `0` marks the last statement in the procedure.
    pub distance: u8,
}

/// A procedure's error handling: every `On Error` and `Resume` it holds.
///
/// Returned by [`PCodeMethod::error_handling`]. The compiler records error
/// handling only in the code (`OnErrorGoto`, `Resume`); the procedure's
/// descriptor has no flag for it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ErrorHandling {
    /// Each `On Error` / `Resume` with its P-Code offset, in code order.
    pub flows: Vec<(u16, ErrorFlow)>,
}

impl ErrorHandling {
    /// Returns the handler labels `On Error GoTo <label>` installs.
    pub fn handlers(&self) -> impl Iterator<Item = u16> + '_ {
        self.flows.iter().filter_map(|(_, flow)| match flow {
            ErrorFlow::OnErrorGoto(label) => Some(*label),
            _ => None,
        })
    }

    /// Returns `true` if the procedure installs a handler (`On Error GoTo
    /// <label>`).
    pub fn has_handler(&self) -> bool {
        self.handlers().next().is_some()
    }

    /// Returns `true` if the procedure has `On Error Resume Next`.
    pub fn resumes_next_on_error(&self) -> bool {
        self.flows
            .iter()
            .any(|(_, flow)| *flow == ErrorFlow::OnErrorResumeNext)
    }

    /// Returns `true` if a handler leaves with `Resume`, `Resume Next` or
    /// `Resume <label>`.
    pub fn resumes(&self) -> bool {
        self.flows.iter().any(|(_, flow)| {
            matches!(
                flow,
                ErrorFlow::Resume | ErrorFlow::ResumeNext | ErrorFlow::ResumeLabel(_)
            )
        })
    }

    /// Returns `true` if the procedure has no `On Error` and no `Resume`.
    pub fn is_empty(&self) -> bool {
        self.flows.is_empty()
    }
}

impl<'a> PCodeMethod<'a> {
    /// Parses a P-Code method from a method table entry.
    ///
    /// The method table at `methods_va` contains 4-byte VA entries. A P-Code
    /// procedure's entry points at its [`ProcDscInfo`] (every fixture), or at
    /// a `mov edx, <ProcDscInfo>` stub (`BA imm32`, or `33 C0 BA imm32`)
    /// from which the address is taken. The P-Code is the `proc_size` bytes
    /// just before the `ProcDscInfo` (`ProcCallEngine` computes the same
    /// start, `ebx - [ebx+8]`, at 0x66104b7f), and the constant pool base is
    /// the owning `ObjectInfo`'s +0x34 (`ProcCallEngine` 0x66104adc).
    ///
    /// # Arguments
    ///
    /// * `map` - Address map for VA-to-offset resolution.
    /// * `methods_va` - Base VA of the method dispatch table.
    /// * `index` - Zero-based slot within the table.
    ///
    /// # Returns
    ///
    /// A [`PCodeMethod`] with the parsed procedure descriptor, the raw
    /// P-Code bytes, and the constant pool base VA.
    ///
    /// # Errors
    ///
    /// Returns an error if any VA in the resolution chain (method table
    /// entry, stub, ProcDscInfo, ObjectInfo, or P-Code region) cannot be
    /// resolved to valid file offsets.
    pub fn parse(map: &AddressMap<'a>, methods_va: u32, index: u16) -> Result<Self, Error> {
        // Each method table entry is 4 bytes (a VA)
        let entry_va = methods_va.wrapping_add(u32::from(index).wrapping_mul(4));
        let entry_data = map.slice_from_va(entry_va, 4)?;
        let method_va = read_u32_le(entry_data, 0)?;

        // The method_va may point to a call stub or directly to ProcDscInfo.
        // Two known P-Code stub patterns:
        //   Pattern 1: BA xx xx xx xx (mov edx, <RTMI>; call ProcCallEngine)
        //   Pattern 2: 33 C0 BA xx xx xx xx 68 xx xx xx xx C3
        //              (xor eax,eax; mov edx, <RTMI>; push <ret>; ret)
        let stub_data = map.slice_from_va(method_va, 12)?;
        let stub_head = stub_data.first_chunk::<3>().ok_or(Error::Truncated {
            needed: 3,
            available: stub_data.len(),
        })?;

        let proc_dsc_va = if stub_head[0] == 0xBA {
            // Pattern 1: mov edx, imm32 at offset 0
            read_u32_le(stub_data, 1)?
        } else if stub_head == &[0x33, 0xC0, 0xBA] {
            // Pattern 2: xor eax,eax; mov edx, imm32 at offset 2
            read_u32_le(stub_data, 3)?
        } else {
            // Assume it's a direct pointer to ProcDscInfo
            method_va
        };

        // Parse ProcDscInfo - read MIN_SIZE first to get total_size, then re-read full
        let pdi_header = map.slice_from_va(proc_dsc_va, ProcDscInfo::MIN_SIZE)?;
        let pdi_tmp = ProcDscInfo::parse(pdi_header)?;
        let full_size = (pdi_tmp.total_size()? as usize).max(ProcDscInfo::MIN_SIZE);
        let pdi_data = map.slice_from_va(proc_dsc_va, full_size)?;
        let proc_dsc = ProcDscInfo::parse(pdi_data)?;

        // P-Code bytes are at [proc_dsc_va - proc_size .. proc_dsc_va]
        let proc_size = proc_dsc.proc_size()?;
        let pcode_va = proc_dsc_va.wrapping_sub(u32::from(proc_size));
        let pcode_data = map.slice_from_va(pcode_va, proc_size as usize)?;
        let pcode_bytes = pcode_data
            .get(..proc_size as usize)
            .ok_or(Error::Truncated {
                needed: proc_size as usize,
                available: pcode_data.len(),
            })?;

        // Get the constant pool base from ObjectInfo.lpConstants (+0x34)
        // ProcDscInfo+0x00 points to ObjectInfo (confirmed via ProcCallEngine_Body)
        let obj_info_va = proc_dsc.object_info_va()?;
        let oi_data = map.slice_from_va(obj_info_va, procedure::OBJECT_INFO_MIN_SIZE)?;
        let data_const_va = procedure::read_constants_va(oi_data)?;

        Ok(Self {
            proc_dsc,
            pcode_bytes,
            data_const_va,
            pcode_va,
            proc_dsc_va,
            stub_va: method_va,
        })
    }

    /// Returns the [`ProcDscInfo`] (RTMI) for this method.
    #[inline]
    pub fn proc_dsc(&self) -> &ProcDscInfo<'a> {
        &self.proc_dsc
    }

    /// Stack frame size for local variables.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying ProcDscInfo field cannot be read.
    #[inline]
    pub fn frame_size(&self) -> Result<u16, Error> {
        self.proc_dsc.frame_size()
    }

    /// Size of the P-Code byte stream in bytes.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying ProcDscInfo field cannot be read.
    #[inline]
    pub fn proc_size(&self) -> Result<u16, Error> {
        self.proc_dsc.proc_size()
    }

    /// Raw P-Code bytes for this method (slice into the file buffer).
    #[inline]
    pub fn pcode_bytes(&self) -> &'a [u8] {
        self.pcode_bytes
    }

    /// Constant pool base VA for resolving string/API references: the
    /// owning `ObjectInfo`'s `constants_va` (+0x34), shared by every method
    /// of the object.
    #[inline]
    pub fn data_const_va(&self) -> u32 {
        self.data_const_va
    }

    /// VA of the first P-Code byte in the PE image.
    ///
    /// This is the address where the P-Code instruction stream begins,
    /// computed as `proc_dsc_va - proc_size`.
    #[inline]
    pub fn pcode_va(&self) -> u32 {
        self.pcode_va
    }

    /// VA of the ProcDscInfo (RTMI) structure in the PE image.
    ///
    /// The ProcDscInfo immediately follows the P-Code byte stream.
    #[inline]
    pub fn proc_dsc_va(&self) -> u32 {
        self.proc_dsc_va
    }

    /// The VA the method table entry holds.
    ///
    /// In every fixture this is the `ProcDscInfo` itself (equal to
    /// [`proc_dsc_va`](Self::proc_dsc_va)), not code. It is a
    /// `mov edx, <ProcDscInfo>` stub only when the entry points at one. The
    /// code that enters a class, form or UserControl method is its
    /// [`MethodLink`](super::MethodLink) stub
    /// (`xor eax, eax; mov edx, <ProcDscInfo>; push <jmp [MethCallEngine]>; ret`),
    /// and `Sub Main`'s is
    /// [`VbHeader::sub_main_va`](crate::vb::header::VbHeader::sub_main_va)
    /// (`mov edx, <ProcDscInfo>; mov ecx, <jmp [ProcCallEngine]>; jmp ecx`).
    #[inline]
    pub fn stub_va(&self) -> u32 {
        self.stub_va
    }

    /// Returns a streaming iterator over decoded P-Code instructions.
    ///
    /// The instruction stream length is bounded by the procedure's on-disk
    /// `u16` `proc_size` field, so a single method can contain at most
    /// 65,535 bytes of P-Code. The decoded instruction count cannot exceed
    /// that byte count because every decoded instruction consumes at least
    /// one byte.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying procedure size cannot be read.
    pub fn instructions(&self) -> Result<InstructionIterator<'a>, Error> {
        Ok(InstructionIterator::new(
            self.pcode_bytes,
            self.proc_dsc.proc_size()?,
        ))
    }

    /// Returns the procedure's beginning-of-statement markers, in order.
    ///
    /// Each `LargeBos` marker ([`Instruction::is_bos`](crate::pcode::decoder::Instruction::is_bos))
    /// starts a source statement, so in a procedure that has them they
    /// partition the instruction stream into statements; most procedures have
    /// none (see [`StatementMarker`]). Each [`StatementMarker`] carries its
    /// P-Code offset and the byte distance to the next boundary (`0` for the
    /// last statement). An instruction that fails to decode is skipped and
    /// decoding goes on after the bytes it consumed.
    ///
    /// # Errors
    ///
    /// Returns an error only if the instruction stream cannot be created
    /// (e.g. the procedure size cannot be read).
    pub fn statement_markers(&self) -> Result<Vec<StatementMarker>, Error> {
        let mut out = Vec::new();
        for insn in self.instructions()?.flatten() {
            if let Some(distance) = insn.bos_distance() {
                out.push(StatementMarker {
                    offset: insn.offset,
                    distance,
                });
            }
        }
        Ok(out)
    }

    /// Returns the procedure's error handling: each `On Error` and `Resume`
    /// ([`Instruction::error_flow`](crate::pcode::decoder::Instruction::error_flow)).
    /// Instructions that fail to decode are skipped.
    ///
    /// # Errors
    ///
    /// Returns an error only if the instruction stream cannot be created.
    pub fn error_handling(&self) -> Result<ErrorHandling, Error> {
        Ok(ErrorHandling {
            flows: self
                .instructions()?
                .flatten()
                .filter_map(|insn| Some((insn.offset, insn.error_flow()?)))
                .collect(),
        })
    }

    /// Iterates the procedure's local-variable cleanup table entries.
    ///
    /// The cleanup table describes resource-release thunks (BSTR free,
    /// VARIANT free, object Release) that the runtime invokes on procedure
    /// exit and on the error path. The table is **P-Code only** -
    /// native-compiled methods emit cleanup calls inline in their x86 code.
    ///
    /// This is a forwarder for [`ProcDscInfo::cleanup_entries`] kept on
    /// [`PCodeMethod`] for ergonomic consumer access; the underlying iterator
    /// is identical.
    #[inline]
    pub fn cleanup_entries(&self) -> ControlPropertyIter<'a> {
        self.proc_dsc.cleanup_entries()
    }

    /// Creates a [`ConstantPool`] reader for this method's constant pool.
    ///
    /// The constant pool is shared by all methods in the same object and
    /// is used to resolve `%s` operands (string literals, API stubs, etc.).
    pub fn constant_pool<'m>(&self, map: &'m AddressMap<'a>) -> ConstantPool<'m>
    where
        'a: 'm,
    {
        ConstantPool::new(map, self.data_const_va)
    }
}
