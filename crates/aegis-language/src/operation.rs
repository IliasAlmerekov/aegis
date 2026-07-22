//! Self-contained detected-operation vocabulary that language adapters emit
//! (ADR-022 §3).
//!
//! `aegis-language` is a workspace leaf and may **not** depend on `aegis-types`
//! (ADR-022 §4, pinned by `tests/aegis_language_boundary.rs`), so the operation
//! vocabulary an adapter *produces* lives here rather than reusing
//! `aegis_types::analysis`. The root `aegis` crate maps these types into
//! `aegis_types::analysis::DetectedOperation` and runs the shared classifier
//! (`aegis_types::analysis::classifier::classify`); no adapter assigns a final
//! `RiskLevel` directly (Iteration 5 REVIEW GATE).
//!
//! This is a deliberate, boundary-forced parallel of `aegis_types::analysis`:
//! `OperationKind`, `OperationModifiers`, `OperandCertainty`, `DetectedOperation`
//! mirror the `aegis-types` enums one-for-one. The duplication is structural —
//! the two crates cannot share the type — and is pinned by a conversion test in
//! the root `aegis` crate's pipeline tests, which assert every variant maps.
//!
//! The types carry no `serde`/`schemars` derives: this is the in-process adapter
//! output, not an audit-persisted record. Audit persistence goes through the
//! `aegis-types` provenance path after the root mapping. Keeping `serde` out
//! preserves the crate's stated dependency invariant (Tree-sitter runtime + four
//! grammars + `thiserror` only — see `tests/aegis_language_boundary.rs`).

use crate::language::SourceLanguage;
use crate::protocol::{DecodeError, EncodeError};

/// The kind of destructive effect or execution sink a language adapter detected
/// (ADR-022 §3 initial scope).
///
/// Mirrors `aegis_types::analysis::OperationKind` one-for-one. `#[non_exhaustive]`
/// so adapters may surface finer-grained kinds without breaking the root mapping.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OperationKind {
    /// Recursive or single filesystem deletion (`os.remove`, …).
    FilesystemDelete,
    /// Overwrite or truncation of an existing file (`open('w')`, …).
    FilesystemOverwrite,
    /// A dangerous permission or ownership change (`os.chown`, …).
    PermissionOrOwnershipChange,
    /// A write to a device file or other critical-path target.
    DeviceOrCriticalWrite,
    /// A destructive database operation.
    DatabaseDestructive,
    /// A recognized process, shell, or eval sink (`subprocess.run`, `eval`, …).
    CodeExecution,
    /// A destructive cloud-provider API call.
    CloudDestructive,
    /// A destructive container-management operation.
    ContainerDestructive,
    /// A destructive package-manager operation.
    PackageDestructive,
}

/// Modifiers that refine an [`OperationKind`] (ADR-022 §3). Mirrors
/// `aegis_types::analysis::OperationModifiers`. All flags default to `false`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct OperationModifiers {
    /// The operation is recursive (`shutil.rmtree`, …).
    pub recursive: bool,
    /// The operation is forced.
    pub forced: bool,
    /// The operation is in an explicitly destructive mode (e.g. a truncating
    /// `open` flag).
    pub destructive_mode: bool,
}

/// How completely a `Detected operation`'s operand is known to static analysis
/// (ADR-022 §3). Mirrors `aegis_types::analysis::OperandCertainty`.
///
/// Ordered by *decreasing* certainty: `Known < Partial < Dynamic`. A `Dynamic`
/// operand is never evidence of safety (ADR-022 §3, §7).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OperandCertainty {
    /// The operand is a literal recoverable from the source.
    Known,
    /// The operand is partially resolved (e.g. an alias or adjacent literal).
    Partial,
    /// The operand is computed, imported, or otherwise not statically recoverable.
    Dynamic,
}

/// A concrete byte span inside analyzed source (ADR-022 §10). Mirrors
/// `aegis_types::analysis::ByteSpan`. Carries position only — never source text.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ByteSpan {
    /// 1-based line number.
    pub line: u32,
    /// 1-based column number (in bytes).
    pub column: u32,
    /// Inclusive start byte offset within the source.
    pub byte_start: usize,
    /// Exclusive end byte offset within the source.
    pub byte_end: usize,
}

/// A literal payload statically recovered from an execution sink, to be enqueued
/// as a bounded recursive analysis target (ADR-022 §7).
///
/// `language` is the payload's *own* language (cross-language: a Python `eval`
/// of a JavaScript literal enqueues a JavaScript target). `source` is the
/// recovered literal body; `span` locates the payload in the parent source for
/// provenance. The root crate wraps this in a `QueueTarget` at `parent_depth+1`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NestedTarget {
    /// The language the payload should be parsed as.
    pub language: SourceLanguage,
    /// The recovered literal payload body.
    pub source: String,
    /// Where the payload was recovered in the parent source.
    pub span: ByteSpan,
}

/// A language-neutral operation detected from source syntax (ADR-022 §3).
///
/// Mirrors `aegis_types::analysis::DetectedOperation` plus an adapter-local
/// `span` (the `aegis-types` type is span-less; the span lives on its
/// `AnalysisProvenance`) and an optional [`NestedTarget`] payload for execution
/// sinks. The root mapping moves `span` into `AnalysisProvenance` and feeds
/// `payload` to the recursive sink invariant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DetectedOperation {
    /// What effect or execution sink was detected.
    pub kind: OperationKind,
    /// Modifiers refining the kind.
    pub modifiers: OperationModifiers,
    /// How completely the operand is known to static analysis.
    pub certainty: OperandCertainty,
    /// Where the operation appears in the source.
    pub span: ByteSpan,
    /// For a `CodeExecution` sink: the statically recovered literal payload, if
    /// any. `None` for non-execution ops and for dynamic/encoded payloads.
    pub payload: Option<NestedTarget>,
}

/// The output of one language adapter over one source target.
///
/// `operations` are the detected destructive effects / execution sinks;
/// `parse_errors` is the count of Tree-sitter `ERROR` nodes in the parse (a
/// nonzero count means the source was malformed, which the root mapping records
/// as `DegradationReason::IncompleteSyntax`). The parent owns status/degradation
/// aggregation and recursive enqueueing (ADR-022 §2).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct AdapterResult {
    /// Detected operations, in source order.
    pub operations: Vec<DetectedOperation>,
    /// Number of Tree-sitter `ERROR` nodes in the parse (0 = clean).
    pub parse_errors: u32,
}

// ── Wire codec (ADR-022 §2) ───────────────────────────────────────────────
//
// The ephemeral worker runs the language adapters and returns each
// `AdapterResult` to the parent over the versioned pipe protocol. `aegis-language`
// is a workspace leaf whose only dependencies are the pinned Tree-sitter runtime,
// the four grammars, and `thiserror` (ADR-022 §8, pinned by
// `tests/aegis_language_boundary.rs`), so the codec is hand-rolled rather than
// serde-derived. The format is packed, little-endian, with no padding:
//
//   AdapterResult:
//     parse_errors: u32 LE
//     op_count:     u32 LE
//     op_count × DetectedOperation
//
//   DetectedOperation:
//     kind:          u8   (OperationKind discriminant, see KIND_*)
//     modifiers:     u8   (bitmask: bit0 recursive, bit1 forced, bit2 destructive_mode)
//     certainty:     u8   (0 Known, 1 Partial, 2 Dynamic)
//     span.line:     u32 LE
//     span.column:   u32 LE
//     span.byte_start: u32 LE
//     span.byte_end: u32 LE
//     has_payload:   u8   (0 or 1)
//     [if 1:] NestedTarget { language u8, source_len u32 LE, source bytes,
//                            span.line u32, column u32, byte_start u32, byte_end u32 }
//
// Offsets and lengths are u32 (a source target is bounded by the protocol's
// 1 MiB ceiling, ADR-022 §7, so they always fit; the encoder is fallible only
// to stay panic-free in production, never truncating with `as u32`).

/// The wire discriminant for [`OperationKind::FilesystemDelete`].
pub const KIND_FS_DELETE: u8 = 0;
/// The wire discriminant for [`OperationKind::FilesystemOverwrite`].
pub const KIND_FS_OVERWRITE: u8 = 1;
/// The wire discriminant for [`OperationKind::PermissionOrOwnershipChange`].
pub const KIND_PERM_OWNER: u8 = 2;
/// The wire discriminant for [`OperationKind::DeviceOrCriticalWrite`].
pub const KIND_DEVICE_WRITE: u8 = 3;
/// The wire discriminant for [`OperationKind::DatabaseDestructive`].
pub const KIND_DB_DESTRUCTIVE: u8 = 4;
/// The wire discriminant for [`OperationKind::CodeExecution`].
pub const KIND_CODE_EXEC: u8 = 5;
/// The wire discriminant for [`OperationKind::CloudDestructive`].
pub const KIND_CLOUD_DESTRUCTIVE: u8 = 6;
/// The wire discriminant for [`OperationKind::ContainerDestructive`].
pub const KIND_CONTAINER_DESTRUCTIVE: u8 = 7;
/// The wire discriminant for [`OperationKind::PackageDestructive`].
pub const KIND_PACKAGE_DESTRUCTIVE: u8 = 8;

/// The wire discriminant for [`OperandCertainty::Known`].
pub const CERT_KNOWN: u8 = 0;
/// The wire discriminant for [`OperandCertainty::Partial`].
pub const CERT_PARTIAL: u8 = 1;
/// The wire discriminant for [`OperandCertainty::Dynamic`].
pub const CERT_DYNAMIC: u8 = 2;

/// The `modifiers` bitmask: bit 0 = recursive, bit 1 = forced, bit 2 = destructive.
pub const MOD_RECURSIVE: u8 = 1 << 0;
/// The `modifiers` bitmask: forced.
pub const MOD_FORCED: u8 = 1 << 1;
/// The `modifiers` bitmask: destructive_mode.
pub const MOD_DESTRUCTIVE: u8 = 1 << 2;

/// Map an [`OperationKind`] to its wire discriminant.
#[must_use]
pub fn kind_to_wire(kind: OperationKind) -> u8 {
    match kind {
        OperationKind::FilesystemDelete => KIND_FS_DELETE,
        OperationKind::FilesystemOverwrite => KIND_FS_OVERWRITE,
        OperationKind::PermissionOrOwnershipChange => KIND_PERM_OWNER,
        OperationKind::DeviceOrCriticalWrite => KIND_DEVICE_WRITE,
        OperationKind::DatabaseDestructive => KIND_DB_DESTRUCTIVE,
        OperationKind::CodeExecution => KIND_CODE_EXEC,
        OperationKind::CloudDestructive => KIND_CLOUD_DESTRUCTIVE,
        OperationKind::ContainerDestructive => KIND_CONTAINER_DESTRUCTIVE,
        OperationKind::PackageDestructive => KIND_PACKAGE_DESTRUCTIVE,
        // `non_exhaustive`: a future adapter kind with no wire code is a build
        // error here rather than a silent mismatch.
    }
}

/// Map a wire discriminant back to an [`OperationKind`], or `None` for an
/// unknown code (a future/invalid value the decoder does not understand).
#[must_use]
pub fn kind_from_wire(code: u8) -> Option<OperationKind> {
    Some(match code {
        KIND_FS_DELETE => OperationKind::FilesystemDelete,
        KIND_FS_OVERWRITE => OperationKind::FilesystemOverwrite,
        KIND_PERM_OWNER => OperationKind::PermissionOrOwnershipChange,
        KIND_DEVICE_WRITE => OperationKind::DeviceOrCriticalWrite,
        KIND_DB_DESTRUCTIVE => OperationKind::DatabaseDestructive,
        KIND_CODE_EXEC => OperationKind::CodeExecution,
        KIND_CLOUD_DESTRUCTIVE => OperationKind::CloudDestructive,
        KIND_CONTAINER_DESTRUCTIVE => OperationKind::ContainerDestructive,
        KIND_PACKAGE_DESTRUCTIVE => OperationKind::PackageDestructive,
        _ => return None,
    })
}

/// Map an [`OperandCertainty`] to its wire discriminant.
#[must_use]
pub fn certainty_to_wire(certainty: OperandCertainty) -> u8 {
    match certainty {
        OperandCertainty::Known => CERT_KNOWN,
        OperandCertainty::Partial => CERT_PARTIAL,
        OperandCertainty::Dynamic => CERT_DYNAMIC,
    }
}

/// Map a wire discriminant back to an [`OperandCertainty`], or `None` for an
/// unknown code. A future `non_exhaustive` certainty decodes as `None`; the
/// caller (the codec) rejects it rather than guessing, so an unknown certainty
/// can never be mislabeled as a safer value.
#[must_use]
pub fn certainty_from_wire(code: u8) -> Option<OperandCertainty> {
    Some(match code {
        CERT_KNOWN => OperandCertainty::Known,
        CERT_PARTIAL => OperandCertainty::Partial,
        CERT_DYNAMIC => OperandCertainty::Dynamic,
        _ => return None,
    })
}

/// Pack an [`OperationModifiers`] into its wire bitmask.
#[must_use]
pub fn modifiers_to_wire(mods: OperationModifiers) -> u8 {
    let mut bits = 0u8;
    if mods.recursive {
        bits |= MOD_RECURSIVE;
    }
    if mods.forced {
        bits |= MOD_FORCED;
    }
    if mods.destructive_mode {
        bits |= MOD_DESTRUCTIVE;
    }
    bits
}

/// Unpack a wire bitmask into [`OperationModifiers`]. Unknown high bits are
/// ignored (forward-compatible: a future modifier the decoder does not know
/// does not corrupt the known three).
#[must_use]
pub fn modifiers_from_wire(bits: u8) -> OperationModifiers {
    OperationModifiers {
        recursive: bits & MOD_RECURSIVE != 0,
        forced: bits & MOD_FORCED != 0,
        destructive_mode: bits & MOD_DESTRUCTIVE != 0,
    }
}

/// Encode an [`AdapterResult`] into its packed little-endian wire form (see the
/// codec section at the top of this module).
///
/// Fallible only so the production path never panics: a `usize` field that does
/// not fit in a `u32` returns [`EncodeError::FieldOverflow`] rather than being
/// truncated with `as u32`. Such a field is unreachable for a source bounded by
/// the protocol's 1 MiB ceiling (ADR-022 §7).
pub fn encode_adapter_result(result: &AdapterResult) -> Result<Vec<u8>, EncodeError> {
    let mut out = Vec::new();
    out.extend_from_slice(&result.parse_errors.to_le_bytes());
    let op_count = u32::try_from(result.operations.len())
        .map_err(|_| EncodeError::FieldOverflow { field: "op_count" })?;
    out.extend_from_slice(&op_count.to_le_bytes());
    for op in &result.operations {
        encode_operation(&mut out, op)?;
    }
    Ok(out)
}

/// Encode one [`DetectedOperation`] into `out`.
fn encode_operation(out: &mut Vec<u8>, op: &DetectedOperation) -> Result<(), EncodeError> {
    out.push(kind_to_wire(op.kind));
    out.push(modifiers_to_wire(op.modifiers));
    out.push(certainty_to_wire(op.certainty));
    encode_span(out, &op.span)?;
    match &op.payload {
        Some(target) => {
            out.push(1);
            // Reuse the protocol's language→tag mapping (single source of truth).
            out.push(crate::protocol::language_to_wire(target.language));
            let len =
                u32::try_from(target.source.len()).map_err(|_| EncodeError::FieldOverflow {
                    field: "payload.source.len",
                })?;
            out.extend_from_slice(&len.to_le_bytes());
            out.extend_from_slice(target.source.as_bytes());
            encode_span(out, &target.span)?;
        }
        None => out.push(0),
    }
    Ok(())
}

/// Encode a [`ByteSpan`] into `out`. `byte_start`/`byte_end` are `usize` on the
/// in-memory type and `u32` on the wire.
fn encode_span(out: &mut Vec<u8>, span: &ByteSpan) -> Result<(), EncodeError> {
    out.extend_from_slice(&span.line.to_le_bytes());
    out.extend_from_slice(&span.column.to_le_bytes());
    let start = u32::try_from(span.byte_start).map_err(|_| EncodeError::FieldOverflow {
        field: "byte_start",
    })?;
    out.extend_from_slice(&start.to_le_bytes());
    let end = u32::try_from(span.byte_end)
        .map_err(|_| EncodeError::FieldOverflow { field: "byte_end" })?;
    out.extend_from_slice(&end.to_le_bytes());
    Ok(())
}

/// Decode an [`AdapterResult`] from its packed little-endian wire form (see the
/// codec section at the top of this module).
///
/// Returns [`DecodeError::InvalidPayload`] for a truncated frame, an unknown
/// kind/certainty/language discriminant, or a length that exceeds the remaining
/// bytes. The decoder never reads past `buf`.
pub fn decode_adapter_result(buf: &[u8]) -> Result<AdapterResult, DecodeError> {
    let mut cur = Cursor::new(buf);
    let parse_errors = cur.read_u32("parse_errors")?;
    let op_count = cur.read_u32("op_count")? as usize;
    // Cap the *initial* allocation so a malformed `op_count` declaring millions
    // of operations cannot drive a huge speculative allocation; the Vec grows
    // as operations actually decode, so this is not a cap on operation count.
    let mut operations = Vec::with_capacity(op_count.min(512));
    for _ in 0..op_count {
        operations.push(decode_operation(&mut cur)?);
    }
    if !cur.is_empty() {
        return Err(DecodeError::InvalidPayload(
            "trailing bytes after AdapterResult",
        ));
    }
    Ok(AdapterResult {
        operations,
        parse_errors,
    })
}

/// Decode one [`DetectedOperation`] from `cur`.
fn decode_operation(cur: &mut Cursor<'_>) -> Result<DetectedOperation, DecodeError> {
    let kind = kind_from_wire(cur.read_u8("kind")?).ok_or(DecodeError::InvalidPayload(
        "unknown operation kind discriminant",
    ))?;
    let modifiers = modifiers_from_wire(cur.read_u8("modifiers")?);
    let certainty = certainty_from_wire(cur.read_u8("certainty")?).ok_or(
        DecodeError::InvalidPayload("unknown certainty discriminant"),
    )?;
    let span = decode_span(cur)?;
    let has_payload = cur.read_u8("has_payload")?;
    let payload = match has_payload {
        0 => None,
        1 => Some(decode_nested_target(cur)?),
        _ => return Err(DecodeError::InvalidPayload("has_payload must be 0 or 1")),
    };
    Ok(DetectedOperation {
        kind,
        modifiers,
        certainty,
        span,
        payload,
    })
}

/// Decode a [`NestedTarget`] (a literal execution-sink payload) from `cur`.
fn decode_nested_target(cur: &mut Cursor<'_>) -> Result<NestedTarget, DecodeError> {
    let language = crate::protocol::wire_to_language(cur.read_u8("payload.language")?)
        .ok_or(DecodeError::InvalidPayload("unknown payload language tag"))?;
    let len = cur.read_u32("payload.source.len")? as usize;
    let source_bytes = cur.read_bytes(len, "payload.source")?;
    let source = String::from_utf8(source_bytes.to_vec())
        .map_err(|_| DecodeError::InvalidPayload("payload source is not valid UTF-8"))?;
    let span = decode_span(cur)?;
    Ok(NestedTarget {
        language,
        source,
        span,
    })
}

/// Decode a [`ByteSpan`] from `cur`.
fn decode_span(cur: &mut Cursor<'_>) -> Result<ByteSpan, DecodeError> {
    let line = cur.read_u32("span.line")?;
    let column = cur.read_u32("span.column")?;
    let byte_start = cur.read_u32("span.byte_start")? as usize;
    let byte_end = cur.read_u32("span.byte_end")? as usize;
    Ok(ByteSpan {
        line,
        column,
        byte_start,
        byte_end,
    })
}

/// A tiny cursor over the input buffer, used by the decoder to read typed
/// fields with bounds checking. Carries a `&'static str` field name in its
/// errors for diagnostics.
struct Cursor<'a> {
    buf: &'a [u8],
}

impl<'a> Cursor<'a> {
    fn new(buf: &'a [u8]) -> Self {
        Self { buf }
    }

    fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    fn read_u8(&mut self, field: &'static str) -> Result<u8, DecodeError> {
        let (head, rest) = self
            .buf
            .split_first()
            .ok_or(DecodeError::InvalidPayload(field))?;
        *self = Self::new(rest);
        Ok(*head)
    }

    fn read_u32(&mut self, field: &'static str) -> Result<u32, DecodeError> {
        if self.buf.len() < 4 {
            return Err(DecodeError::InvalidPayload(field));
        }
        let value = u32::from_le_bytes(self.buf[..4].try_into().expect("checked len"));
        *self = Self::new(&self.buf[4..]);
        Ok(value)
    }

    fn read_bytes(&mut self, len: usize, field: &'static str) -> Result<&'a [u8], DecodeError> {
        if self.buf.len() < len {
            return Err(DecodeError::InvalidPayload(field));
        }
        let (head, rest) = self.buf.split_at(len);
        *self = Self::new(rest);
        Ok(head)
    }
}

#[cfg(test)]
#[path = "operation_tests.rs"]
mod tests;
