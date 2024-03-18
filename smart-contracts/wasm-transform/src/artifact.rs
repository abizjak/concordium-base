//! This module defines the notion of the [`Artifact`], which is a processed and
//! instantiated module that can have its exposed methods invoked via the
//! [`Artifact::run`] method.
//!
//! The module in this section is in a format where serialization and
//! deserialization are straightforward and cheap.

use crate::{
    constants::MAX_NUM_PAGES,
    types::*,
    validate::{validate, Handler, HasValidationContext, LocalsRange, ValidationState},
};
use anyhow::{anyhow, bail, ensure, Context};
use derive_more::{Display, From, Into};
use std::{
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet, BinaryHeap},
    convert::{TryFrom, TryInto},
    io::Write,
    sync::Arc,
};

#[derive(Copy, Clone)]
/// Either a short or long integer.
/// The reason this is a union is that after Wasm module validation we are
/// guaranteed that the program is well typed. Since all instructions have
/// clear, fixed types, we can determine from the instruction which value we
/// expect on the stack. Using a union saves on the discriminant compared to
/// using an enum, leading to 50% less space used on the stack, as well as
/// removes the need to handle impossible cases.
#[repr(C)]
pub union StackValue {
    pub short: i32,
    pub long:  i64,
}

/// The debug implementation does not print the actual value. Instead it always
/// displays `StackValue`. It exists so that structures containing stack values
/// can have useful [`Debug`] implementations.
impl std::fmt::Debug for StackValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result { f.write_str("StackValue") }
}

impl From<i32> for StackValue {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn from(short: i32) -> Self {
        Self {
            short,
        }
    }
}

impl From<u32> for StackValue {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn from(short: u32) -> Self {
        Self {
            short: short as i32,
        }
    }
}

impl From<i64> for StackValue {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn from(long: i64) -> Self {
        Self {
            long,
        }
    }
}

impl From<u64> for StackValue {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn from(long: u64) -> Self {
        Self {
            long: long as i64,
        }
    }
}

impl From<GlobalInit> for StackValue {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn from(g: GlobalInit) -> Self {
        match g {
            GlobalInit::I32(short) => Self {
                short,
            },
            GlobalInit::I64(long) => Self {
                long,
            },
        }
    }
}

#[derive(Debug, Clone)]
/// A fully instantiated table. This is possible because in the Wasm
/// specification we have, the only way to write functions to the table is via
/// the elements section of the module. Since we ensure the table is small
/// enough we can afford to initialize it at compile time.
pub struct InstantiatedTable {
    pub functions: Vec<Option<FuncIndex>>,
}

#[derive(Debug, Clone)]
/// Fully instantiated globals with initial values.
pub struct InstantiatedGlobals {
    pub inits: Vec<GlobalInit>,
}

#[derive(Debug, Clone)]
/// The data segment of the artifact. This is a slightly processed
/// data segment of the module. In contrast to the table we cannot use
/// the same trick of initializing it here. In practice data segments
/// are at high offsets, which would lead to big artifacts. Thus we
/// store it pretty much in the same way that it was when it was part
/// of the source, except we have resolved the offset.
pub struct ArtifactData {
    /// Where to start initializing.
    pub offset: i32,
    /// The bytes to initialize with.
    pub init:   Vec<u8>,
}

impl From<Data> for ArtifactData {
    fn from(d: Data) -> Self {
        Self {
            offset: d.offset,
            init:   d.init,
        }
    }
}

#[derive(Debug, Clone)]
/// Memory of the artifact, with initial size, as well as maximum size set.
/// If the maximum size is not part of the original module we set it to the
/// [constants::MAX_NUM_PAGES](../constants/constant.MAX_NUM_PAGES.html)
pub struct ArtifactMemory {
    pub init_size: u32,
    pub max_size:  u32,
    pub init:      Vec<ArtifactData>,
}

/// A local variable declaration in a function.
/// Because we know there are not going to be more than 2^16-1 locals we can
/// store multiplicity more efficiently.
#[derive(Debug, Clone, Copy)]
pub struct ArtifactLocal {
    pub(crate) multiplicity: u16,
    pub(crate) ty:           ValueType,
}

impl From<ValueType> for ArtifactLocal {
    fn from(ty: ValueType) -> Self {
        Self {
            ty,
            multiplicity: 1,
        }
    }
}

impl TryFrom<Local> for ArtifactLocal {
    type Error = anyhow::Error;

    fn try_from(value: Local) -> Result<Self, Self::Error> {
        let multiplicity = value.multiplicity.try_into()?;
        Ok(Self {
            ty: value.ty,
            multiplicity,
        })
    }
}

#[derive(Debug, Clone)]
/// A function which has been processed into a form suitable for execution.
pub struct CompiledFunction {
    type_idx:      TypeIndex,
    return_type:   BlockType,
    /// Parameters of the function.
    params:        Vec<ValueType>,
    /// Number of locals, cached, but should match what is in the
    /// locals vector below.
    num_locals:    u32,
    /// Vector of types of locals. This __does not__ include function
    /// parameters.
    locals:        Vec<ArtifactLocal>,
    /// Maximum number of locations needed. This includes parameters,
    /// locals, and any extra locations needed to preserve values.
    num_registers: u32,
    /// The constants in the function.
    constants:     Vec<i64>,
    code:          Instructions,
}

#[derive(Debug)]
/// A borrowed variant of [CompiledFunction](./struct.CompiledFunction.html)
/// that does not own the body and locals. This is used to make deserialization
/// of artifacts cheaper.
pub struct CompiledFunctionBytes<'a> {
    pub(crate) type_idx:      TypeIndex,
    pub(crate) return_type:   BlockType,
    pub(crate) params:        &'a [ValueType],
    /// Vector of types of locals. This __does not__ include
    /// parameters.
    /// FIXME: It would be ideal to have this as a zero-copy structure,
    /// but it likely does not matter, and it would be more error-prone.
    pub(crate) num_locals:    u32,
    pub(crate) locals:        Vec<ArtifactLocal>,
    /// Maximum number of locations needed. This includes parameters,
    /// locals, and any extra locations needed to preserve values.
    pub(crate) num_registers: u32,
    /// The constants in the function.
    pub(crate) constants:     Vec<i64>, // TODO: Would be better if it was not allocated.
    pub(crate) code:          &'a [u8],
}

impl<'a> From<CompiledFunctionBytes<'a>> for CompiledFunction {
    fn from(cfb: CompiledFunctionBytes<'a>) -> Self {
        Self {
            type_idx:      cfb.type_idx,
            return_type:   cfb.return_type,
            params:        cfb.params.to_vec(),
            num_locals:    cfb.num_locals,
            locals:        cfb.locals,
            num_registers: cfb.num_registers,
            constants:     cfb.constants,
            code:          cfb.code.to_vec().into(),
        }
    }
}

/// Try to process an import into something that is perhaps more suitable for
/// execution, i.e., quicker to resolve.
pub trait TryFromImport: Sized {
    fn try_from_import(ty: &[FunctionType], import: Import) -> CompileResult<Self>;
    fn ty(&self) -> &FunctionType;
}

/// An example of a processed import with minimal processing. Useful for testing
/// and experimenting, but not for efficient execution.
#[derive(Debug, Clone, Display)]
#[display(fmt = "{}.{}", mod_name, item_name)]
pub struct ArtifactNamedImport {
    pub(crate) mod_name:  Name,
    pub(crate) item_name: Name,
    pub(crate) ty:        FunctionType,
}

impl ArtifactNamedImport {
    pub fn matches(&self, mod_name: &str, item_name: &str) -> bool {
        self.mod_name.as_ref() == mod_name && self.item_name.as_ref() == item_name
    }
}

impl TryFromImport for ArtifactNamedImport {
    fn try_from_import(ty: &[FunctionType], import: Import) -> CompileResult<Self> {
        match import.description {
            ImportDescription::Func {
                type_idx,
            } => {
                let ty = ty
                    .get(type_idx as usize)
                    .ok_or_else(|| anyhow!("Unknown type index."))?
                    .clone();
                Ok(Self {
                    mod_name: import.mod_name,
                    item_name: import.item_name,
                    ty,
                })
            }
        }
    }

    fn ty(&self) -> &FunctionType { &self.ty }
}

/// An iterator over local variables.
pub struct LocalsIterator<'a> {
    /// Number of locals that are still going to be yielded from the iterator.
    remaining_items:      u32,
    pub(crate) locals:    &'a [ArtifactLocal],
    /// Current position in the locals list. Each local in the list can have a
    /// multiplicity. This is the shorthand Wasm uses for declaring multiple
    /// local variables of the same type.
    current_item:         usize,
    /// Current multiplicity of the `current_item`.
    /// When advancing the iterator we keep increasing this until we exhaust the
    /// local.
    current_multiplicity: u16,
}

impl<'a> LocalsIterator<'a> {
    /// Construct a new iterator given the total number of locals and a list of
    /// locals with multiplicity. The total number of locals must be supplied so
    /// that we don't have to go through the entire list of locals and sum up
    /// their multiplicities.
    pub fn new(num_locals: u32, locals: &'a [ArtifactLocal]) -> Self {
        Self {
            remaining_items: num_locals,
            locals,
            current_item: 0,
            current_multiplicity: 0,
        }
    }
}

impl<'a> Iterator for LocalsIterator<'a> {
    type Item = ValueType;

    fn next(&mut self) -> Option<Self::Item> {
        self.remaining_items.checked_sub(1)?;
        let al = self.locals.get(self.current_item)?;
        if self.current_multiplicity < al.multiplicity {
            self.current_multiplicity += 1;
            Some(al.ty)
        } else {
            self.current_item += 1;
            self.current_multiplicity = 0;
            self.next()
        }
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        (self.remaining_items as usize, Some(self.remaining_items as usize))
    }
}

impl<'a> ExactSizeIterator for LocalsIterator<'a> {
    fn len(&self) -> usize { self.remaining_items as usize }
}

/// A trait encapsulating the properties that are needed to run a function.
/// This trait exists because we have two different kinds of code we run. A
/// fully deserialized code, i.e., where instructions are essentially
/// `Vec<InternalOpcode>` or we execute directly from `&[u8]` if the origin of
/// the code is a serialized structure, such as an [`Artifact`] retrieved from a
/// database.
pub trait RunnableCode {
    /// The number of parameters of the function.
    fn num_params(&self) -> u32;
    /// The number of registers the function needs in the worst case.
    fn num_registers(&self) -> u32;
    /// The number of registers the function needs in the worst case.
    fn constants(&self) -> &[i64];
    /// The type of the function, as an index into the list of types of the
    /// module.
    fn type_idx(&self) -> TypeIndex;
    /// The return type of the function.
    fn return_type(&self) -> BlockType;
    /// The types of function parameters.
    fn params(&self) -> &[ValueType];
    /// The number of locals declared by the function. This **does not** include
    /// the function parameters, only declared locals.
    fn num_locals(&self) -> u32;
    /// An iterator over the locals (not including function parameters).
    fn locals(&self) -> LocalsIterator<'_>;
    /// A reference to the instructions to execute.
    fn code(&self) -> &[u8];
}

impl RunnableCode for CompiledFunction {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_params(&self) -> u32 { self.params.len() as u32 }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_registers(&self) -> u32 { self.num_registers }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn constants(&self) -> &[i64] { &self.constants }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn type_idx(&self) -> TypeIndex { self.type_idx }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn return_type(&self) -> BlockType { self.return_type }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn params(&self) -> &[ValueType] { &self.params }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_locals(&self) -> u32 { self.num_locals }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn locals(&self) -> LocalsIterator { LocalsIterator::new(self.num_locals, &self.locals) }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn code(&self) -> &[u8] { &self.code.bytes }
}

impl<'a> RunnableCode for CompiledFunctionBytes<'a> {
    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_params(&self) -> u32 { self.params.len() as u32 }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_registers(&self) -> u32 { self.num_registers }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn constants(&self) -> &[i64] { &self.constants }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn type_idx(&self) -> TypeIndex { self.type_idx }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn return_type(&self) -> BlockType { self.return_type }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn params(&self) -> &[ValueType] { self.params }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn num_locals(&self) -> u32 { self.num_locals }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn locals(&self) -> LocalsIterator { LocalsIterator::new(self.num_locals, &self.locals) }

    #[cfg_attr(not(feature = "fuzz-coverage"), inline(always))]
    fn code(&self) -> &[u8] { self.code }
}

/// A processed Wasm module. This no longer has custom sections since they are
/// not needed for further processing.
/// The type parameter `ImportFunc` is instantiated with the representation of
/// host functions. To efficiently and relatively safely execute the module we
/// preprocess imported functions into an enum. However for testing we sometimes
/// just use raw imports. This type parameter allows us flexibility.
///
/// The type parameter `CompiledCode` is used to allow flexibility in code
/// representation. For testing uses it is convenient that the type is
/// "owned", in the sense of it being a vector of instructions. For efficient
/// execution, and to avoid deserialization, the code is represented as a byte
/// array (i.e., as a slice of bytes `&[u8]`) when we execute it after looking
/// the code up from the database.
#[derive(Debug, Clone)]
pub struct Artifact<ImportFunc, CompiledCode> {
    /// Imports by (module name, item name).
    pub imports: Vec<ImportFunc>,
    /// Types of the module. These are needed for dynamic dispatch, i.e.,
    /// call-indirect.
    pub ty:      Vec<FunctionType>,
    /// A fully instantiated table.
    pub table:   InstantiatedTable,
    /// The memory of the artifact.
    pub memory:  Option<ArtifactMemory>,
    /// Globals initialized with initial values.
    pub global:  InstantiatedGlobals,
    /// The exported functions.
    /// Validation should ensure that an exported function is a defined one,
    /// and not one of the imported ones.
    /// Thus the index refers to the index in the code section.
    pub export:  BTreeMap<Name, FuncIndex>,
    /// The list of functions in the module.
    pub code:    Vec<CompiledCode>,
}

/// Ar artifact which does not own the code to run. The code is only a reference
/// to a byte array.
pub type BorrowedArtifact<'a, ImportFunc> = Artifact<ImportFunc, CompiledFunctionBytes<'a>>;
/// An artifact that owns the code to run.
pub type OwnedArtifact<ImportFunc> = Artifact<ImportFunc, CompiledFunction>;

/// Convert a borrowed artifact to an owned one. This allocates memory for all
/// the code of the artifact so it should be used sparingly.
impl<'a, ImportFunc> From<BorrowedArtifact<'a, ImportFunc>> for OwnedArtifact<ImportFunc> {
    fn from(a: BorrowedArtifact<'a, ImportFunc>) -> Self {
        let Artifact {
            imports,
            ty,
            table,
            memory,
            global,
            export,
            code,
        } = a;
        Self {
            imports,
            ty,
            table,
            memory,
            global,
            export,
            code: code.into_iter().map(CompiledFunction::from).collect::<Vec<_>>(),
        }
    }
}

/// Convert a borrowed artifact to an owned one inside an `Arc`. This allocates
/// memory for all the code of the artifact so it should be used sparingly.
impl<'a, ImportFunc> From<BorrowedArtifact<'a, ImportFunc>> for Arc<OwnedArtifact<ImportFunc>> {
    fn from(a: BorrowedArtifact<'a, ImportFunc>) -> Self { Arc::new(a.into()) }
}

/// Internal opcode. This is mostly the same as [`OpCode`], but with control
/// instructions resolved to jumps in the instruction sequence, and function
/// calls processed.
#[repr(u8)]
#[derive(Debug, num_enum::TryFromPrimitive)]
pub enum InternalOpcode {
    // Control instructions
    Unreachable = 0u8,
    If,
    Br,
    BrIf,
    BrTable,
    BrTableCarry,
    Return,
    Call,
    CallImmediate,
    CallIndirect,

    // Parametric instructions
    Select,

    // Variable instructions
    LocalSet,
    GlobalGet,
    GlobalSet,

    // Memory instructions
    I32Load,
    I64Load,
    I32Load8S,
    I32Load8U,
    I32Load16S,
    I32Load16U,
    I64Load8S,
    I64Load8U,
    I64Load16S,
    I64Load16U,
    I64Load32S,
    I64Load32U,
    I32Store,
    I64Store,
    I32Store8,
    I32Store16,
    I64Store8,
    I64Store16,
    I64Store32,
    MemorySize,
    MemoryGrow,

    I32Eqz,
    I32Eq,
    I32Ne,
    I32LtS,
    I32LtU,
    I32GtS,
    I32GtU,
    I32LeS,
    I32LeU,
    I32GeS,
    I32GeU,
    I64Eqz,
    I64Eq,
    I64Ne,
    I64LtS,
    I64LtU,
    I64GtS,
    I64GtU,
    I64LeS,
    I64LeU,
    I64GeS,
    I64GeU,

    I32Clz,
    I32Ctz,
    I32Popcnt,
    I32Add,
    I32Sub,
    I32Mul,
    I32DivS,
    I32DivU,
    I32RemS,
    I32RemU,
    I32And,
    I32Or,
    I32Xor,
    I32Shl,
    I32ShrS,
    I32ShrU,
    I32Rotl,
    I32Rotr,
    I64Clz,
    I64Ctz,
    I64Popcnt,
    I64Add,
    I64Sub,
    I64Mul,
    I64DivS,
    I64DivU,
    I64RemS,
    I64RemU,
    I64And,
    I64Or,
    I64Xor,
    I64Shl,
    I64ShrS,
    I64ShrU,
    I64Rotl,
    I64Rotr,

    I32WrapI64,
    I64ExtendI32S,
    I64ExtendI32U,

    // Sign extension instructions, optionally supported depending on the protocol version.
    I32Extend8S,
    I32Extend16S,
    I64Extend8S,
    I64Extend16S,
    I64Extend32S,

    Copy,
}

/// Result of compilation. Either Ok(_) or an error indicating the reason.
pub type CompileResult<A> = anyhow::Result<A>;

#[derive(Default, Debug, Clone, From, Into)]
/// A sequence of internal opcodes, followed by any immediate arguments.
pub struct Instructions {
    pub(crate) bytes: Vec<u8>,
}

impl Instructions {
    fn push(&mut self, opcode: InternalOpcode) { self.bytes.push(opcode as u8) }

    fn push_u16(&mut self, x: u16) { self.bytes.extend_from_slice(&x.to_le_bytes()); }

    fn push_u32(&mut self, x: u32) { self.bytes.extend_from_slice(&x.to_le_bytes()); }

    fn push_i32(&mut self, x: i32) { self.bytes.extend_from_slice(&x.to_le_bytes()); }

    fn current_offset(&self) -> usize { self.bytes.len() }

    fn back_patch(&mut self, back_loc: usize, to_write: u32) -> CompileResult<()> {
        let mut place: &mut [u8] = &mut self.bytes[back_loc..];
        place.write_all(&to_write.to_le_bytes())?;
        Ok(())
    }
}

/// Target of a jump that we need to keep track of temporarily.
enum JumpTarget {
    /// We know the position in the instruction sequence where the jump should
    /// resolve to. This is used in the case of loops.
    Known {
        pos:    usize,
        result: Option<Provider>,
    },
    /// We do not yet know where in the instruction sequence we will jump to.
    /// We record the list of places at which we need to back-patch the location
    /// when we get to it.
    Unknown {
        backpatch_locations: Vec<usize>,
        result:              Option<Provider>,
    },
}

impl JumpTarget {
    pub fn result_slot(&self) -> Option<Provider> {
        match self {
            JumpTarget::Known {
                result,
                ..
            } => *result,
            JumpTarget::Unknown {
                result,
                ..
            } => *result,
        }
    }

    pub fn new_unknown(result: Option<Provider>) -> Self {
        JumpTarget::Unknown {
            backpatch_locations: Vec::new(),
            result,
        }
    }

    pub fn new_unknown_loc(pos: usize, result: Option<Provider>) -> Self {
        JumpTarget::Unknown {
            backpatch_locations: vec![pos],
            result,
        }
    }

    pub fn new_known(pos: usize, result: Option<Provider>) -> Self {
        JumpTarget::Known {
            pos,
            result,
        }
    }
}

#[derive(Default)]
/// Stack of jump targets
struct BackPatchStack {
    stack: Vec<JumpTarget>,
}

impl BackPatchStack {
    pub fn push(&mut self, target: JumpTarget) { self.stack.push(target) }

    pub fn pop(&mut self) -> CompileResult<JumpTarget> {
        self.stack.pop().ok_or_else(|| anyhow!("Attempt to pop from an empty stack."))
    }

    pub fn get_mut(&mut self, n: LabelIndex) -> CompileResult<&mut JumpTarget> {
        ensure!(
            (n as usize) < self.stack.len(),
            "Attempt to access label beyond the size of the stack."
        );
        let lookup_idx = self.stack.len() - n as usize - 1;
        self.stack.get_mut(lookup_idx).ok_or_else(|| anyhow!("Attempt to access unknown label."))
    }

    pub fn get(&self, n: LabelIndex) -> CompileResult<&JumpTarget> {
        ensure!(
            (n as usize) < self.stack.len(),
            "Attempt to access label beyond the size of the stack."
        );
        let lookup_idx = self.stack.len() - n as usize - 1;
        self.stack.get(lookup_idx).ok_or_else(|| anyhow!("Attempt to access unknown label."))
    }
}

struct DynamicLocations {
    next_location:      i32,
    reusable_locations: BTreeSet<i32>,
}

impl DynamicLocations {
    pub fn new(next_location: i32) -> Self {
        Self {
            next_location,
            reusable_locations: BTreeSet::new(),
        }
    }

    pub fn reuse(&mut self, provider: Provider) {
        if let Provider::Dynamic(idx) = provider {
            self.reusable_locations.insert(idx);
        }
    }

    pub fn get(&mut self) -> i32 {
        if let Some(idx) = self.reusable_locations.pop_first() {
            idx
        } else {
            let idx = self.next_location;
            self.next_location += 1;
            idx
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
enum Provider {
    Dynamic(i32),
    Local(i32),
    Constant(i32),
}

/// An intermediate structure of the instruction sequence plus any pending
/// backpatch locations we need to resolve.
struct BackPatch {
    out:               Instructions,
    backpatch:         BackPatchStack,
    providers_stack:   Vec<Provider>,
    /// The return type of the function.
    return_type:       Option<ValueType>,
    dynamic_locations: DynamicLocations,
    constants:         BTreeMap<i64, i32>,
    /// If the last instruction produced something
    /// in the dynamic area record the location here
    /// so we can short-circuit the LocalSet that immediately
    /// follows such an instruction.
    last_provide_loc:  Option<usize>,
}

impl BackPatch {
    /// Construct a new instance. The argument is the number of locals (this
    /// includes parameters + declared locals). The number of locals is
    /// assumed to be within bounds ensured by the validation.
    fn new(num_locals: u32, return_type: Option<ValueType>) -> Self {
        let dynamic_locations = DynamicLocations::new(num_locals as i32 + 1);
        Self {
            out: Default::default(),
            backpatch: BackPatchStack {
                // The return value of a function, if any, will always be at index 0.
                stack: vec![JumpTarget::new_unknown(return_type.map(|_| Provider::Local(0)))],
            },
            providers_stack: Vec::new(),
            constants: BTreeMap::new(),
            dynamic_locations,
            return_type,
            last_provide_loc: None,
        }
    }

    fn push_loc(&mut self, loc: Provider) {
        // TODO: Record preserve locations.
        match loc {
            Provider::Dynamic(idx) => {
                self.out.push_i32(idx);
            }
            Provider::Constant(idx) => {
                self.out.push_i32(idx);
            }
            Provider::Local(idx) => {
                self.out.push_i32(idx);
            }
        }
    }

    fn push_jump(
        &mut self,
        label_idx: LabelIndex,
        state: &ValidationState,
        old_stack_height: usize, // stack height before the jump
        instruction: Option<InternalOpcode>,
    ) -> CompileResult<()> {
        let target_frame = state
            .ctrls
            .get(label_idx)
            .ok_or_else(|| anyhow!("Could not get jump target frame."))?;
        let target_height = target_frame.height;
        ensure!(
            old_stack_height >= target_height,
            "Current height must be at least as much as the target, {} >= {}",
            old_stack_height,
            target_height
        );
        let target = self.backpatch.get(label_idx)?;
        match target {
            JumpTarget::Known {
                pos: _,
                result: _, // when jumping to a loop we don't carry any value over.
            } => (),
            JumpTarget::Unknown {
                backpatch_locations: _,
                result,
            } => {
                if matches!(target_frame.label_type, BlockType::ValueType(_)) {
                    let &Some(result) = result else {
                        anyhow::bail!("No target to place result.");
                    };
                    // If or Br instruction.
                    // output instruction
                    if instruction.is_some() {
                        let provider = self
                            .providers_stack
                            .pop()
                            .context("Expected a value at the top of the stack to carry over.")?;
                        if provider != result {
                            self.out.push(InternalOpcode::Copy);
                            self.push_loc(provider);
                            self.push_loc(result);
                        }
                        self.providers_stack.push(result);
                    } else {
                        // BrTable instruction.
                        self.push_loc(result);
                        // Since the part after the BrTable is unreachable.
                        // the value of the providers stack does not matter.
                    }
                }
            }
        }
        if let Some(i) = instruction {
            self.out.push(i);
        };
        let target = self.backpatch.get_mut(label_idx)?;
        match target {
            JumpTarget::Known {
                pos,
                ..
            } => {
                self.out.push_u32(*pos as u32);
            }
            JumpTarget::Unknown {
                backpatch_locations,
                ..
            } => {
                // output a dummy value
                backpatch_locations.push(self.out.current_offset());
                self.out.push_u32(0u32);
            }
        }
        Ok(())
    }

    fn push_binary(&mut self, opcode: InternalOpcode) -> CompileResult<()> {
        self.out.push(opcode);
        let _ = self.push_consume()?;
        let _ = self.push_consume()?;
        self.push_provide();
        Ok(())
    }

    fn push_ternary(&mut self, opcode: InternalOpcode) -> CompileResult<()> {
        self.out.push(opcode);
        let _ = self.push_consume()?;
        let _ = self.push_consume()?;
        let _ = self.push_consume()?;
        self.push_provide();
        Ok(())
    }

    fn push_mem_load(&mut self, opcode: InternalOpcode, memarg: &MemArg) -> CompileResult<()> {
        self.out.push(opcode);
        self.out.push_u32(memarg.offset);
        let _operand = self.push_consume()?;
        self.push_provide();
        Ok(())
    }

    fn push_mem_store(&mut self, opcode: InternalOpcode, memarg: &MemArg) -> CompileResult<()> {
        self.out.push(opcode);
        self.out.push_u32(memarg.offset);
        let _value = self.push_consume()?;
        let _location = self.push_consume()?;
        Ok(())
    }

    fn push_unary(&mut self, opcode: InternalOpcode) -> CompileResult<()> {
        self.out.push(opcode);
        let _operand = self.push_consume()?;
        self.push_provide();
        Ok(())
    }

    fn push_provide(&mut self) {
        let result = self.dynamic_locations.get();
        self.providers_stack.push(Provider::Dynamic(result));
        self.last_provide_loc = Some(self.out.current_offset());
        self.out.push_i32(result);
    }

    /// Consume an operand and return the slot that was consumed.
    fn push_consume(&mut self) -> CompileResult<Provider> {
        let operand = self
            .providers_stack
            .pop()
            .with_context(|| format!("Missing operand for push_consume"))?;
        self.dynamic_locations.reuse(operand);
        self.push_loc(operand);
        Ok(operand)
    }
}

impl<Ctx: HasValidationContext> Handler<Ctx, &OpCode> for BackPatch {
    type Outcome = (Instructions, i32, Vec<i64>);

    fn handle_opcode(
        &mut self,
        ctx: &Ctx,
        state: &ValidationState,
        stack_height: usize,
        unreachable_before: bool,
        opcode: &OpCode,
    ) -> CompileResult<()> {
        use InternalOpcode::*;
        let last_provide = self.last_provide_loc.take();
        if unreachable_before && !matches!(opcode, OpCode::End | OpCode::Else) {
            return Ok(());
        }
        match opcode {
            OpCode::End => {
                // dbg!("LEN = {}", state.opds.stack.len());
                // dbg!("LEN = {}", self.providers_stack.len());
                let jump_target = self.backpatch.pop()?;
                // Insert an additional Copy instruction if the top of the provider stack
                // does not match what is expected.
                if let Some(result) = jump_target.result_slot() {
                    // if we are in an unreachable segment then
                    // the stack might be empty at this point, and in general
                    // there is no point in inserting a copy instruction
                    // since it'll never be executed.
                    if !unreachable_before {
                        let provider = self
                            .providers_stack
                            .pop()
                            .context("Expected a value at the top of the stack to end with.")?;
                        if provider != result {
                            self.out.push(InternalOpcode::Copy);
                            self.push_loc(provider);
                            self.push_loc(result);
                        }
                    } else {
                        self.providers_stack.truncate(state.opds.stack.len());
                        // There might not actually be anything at the top of the stack
                        // in the unreachable segment. But there might, in which case
                        // we must remove it to make sure that the `result` is at the top
                        // after the block ends.
                        if self.providers_stack.len() == state.opds.stack.len() {
                            self.providers_stack.pop();
                        }
                    }
                    self.providers_stack.push(result);
                } else {
                    self.providers_stack.truncate(state.opds.stack.len());
                }
                if let JumpTarget::Unknown {
                    backpatch_locations,
                    result,
                } = jump_target
                {
                    // As u32 would be safe here since module sizes are much less than 4GB, but
                    // we are being extra careful.
                    let current_pos: u32 = self.out.bytes.len().try_into()?;
                    for pos in backpatch_locations {
                        self.out.back_patch(pos, current_pos)?;
                    }
                } // else if it is a Known location there is no backpatching
                  // needed. And no need to emit any code,
                  // since end is implicit in generated code
                  //
            }
            OpCode::Block(ty) => {
                let result = if matches!(ty, BlockType::ValueType(_)) {
                    let r = self.dynamic_locations.get();
                    Some(r)
                } else {
                    None
                };
                self.backpatch.push(JumpTarget::new_unknown(result.map(Provider::Dynamic)));
            }
            OpCode::Loop(ty) => {
                let result = if matches!(ty, BlockType::ValueType(_)) {
                    let r = self.dynamic_locations.get();
                    Some(r)
                } else {
                    None
                };
                self.backpatch.push(JumpTarget::new_known(
                    self.out.current_offset(),
                    result.map(Provider::Dynamic),
                ))
            }
            OpCode::If {
                ty,
            } => {
                self.out.push(If);
                self.push_consume()?;
                let result = if matches!(ty, BlockType::ValueType(_)) {
                    let r = self.dynamic_locations.get();
                    Some(r)
                } else {
                    None
                };
                self.backpatch.push(JumpTarget::new_unknown_loc(
                    self.out.current_offset(),
                    result.map(Provider::Dynamic),
                ));
                self.out.push_u32(0);
            }
            OpCode::Else => {
                // If we reached the else normally, after executing the if branch, we just break
                // to the end of else.
                self.push_jump(0, state, stack_height, Some(Br))?;
                // Because the module is well-formed this can only happen after an if
                // We do not backpatch the code now, apart from the initial jump to the else
                // branch. The effect of this will be that any break out of the if statement
                // will jump to the end of else, as intended.
                if let JumpTarget::Unknown {
                    backpatch_locations,
                    result,
                } = self.backpatch.get_mut(0)?
                {
                    // As u32 would be safe here since module sizes are much less than 4GB, but
                    // we are being extra careful.
                    let current_pos: u32 = self.out.bytes.len().try_into()?;
                    ensure!(
                        !backpatch_locations.is_empty(),
                        "Backpatch should contain at least the If start."
                    );
                    let first = backpatch_locations.remove(0);
                    self.out.back_patch(first, current_pos)?;

                    // TODO: This is duplicated from the End
                    if let &mut Some(result) = result {
                        // if we are in an unreachable segment then
                        // the stack might be empty at this point, and in general
                        // there is no point in inserting a copy instruction
                        // since it'll never be executed.
                        if !unreachable_before {
                            let provider = self
                                .providers_stack
                                .pop()
                                .context("Expected a value at the top of the stack to end with.")?;
                            if provider != result {
                                self.out.push(InternalOpcode::Copy);
                                self.push_loc(provider);
                                self.push_loc(result);
                            }
                        } else {
                            self.providers_stack.truncate(state.opds.stack.len());
                            // There might not actually be anything at the top of the stack
                            // in the unreachable segment. But there might, in which case
                            // we must remove it to make sure that the `result` is at the top
                            // after the block ends.
                            self.providers_stack.pop();
                        }
                        self.providers_stack.push(result);
                    } else {
                        self.providers_stack.truncate(state.opds.stack.len());
                    }
                } else {
                    bail!("Invariant violation in else branch.")
                }
            }
            OpCode::Br(label_idx) => {
                self.push_jump(*label_idx, state, stack_height, Some(Br))?;
            }
            OpCode::BrIf(label_idx) => {
                // TODO: We output first the target and then the conditional source. This is
                // maybe not ideal since the conditional will sometimes not be
                // taken in which case we don't need to read that.
                let condition_source =
                    self.providers_stack.pop().context("BrIf requires a provider.")?;
                self.push_jump(*label_idx, state, stack_height, Some(BrIf))?;
                self.push_loc(condition_source);
            }
            OpCode::BrTable {
                labels,
                default,
            } => {
                let target_frame =
                    state.ctrls.get(*default).context("Could not get jump target frame.")?;
                if let BlockType::EmptyType = target_frame.label_type {
                    self.out.push(BrTable);
                    let _condition_source = self.push_consume()?;
                } else {
                    self.out.push(BrTableCarry);
                    let _condition_source = self.push_consume()?;
                    let _copy_source = self.push_consume()?;
                }

                // the try_into is not needed because MAX_SWITCH_SIZE is small enough
                // but it does not hurt.
                let labels_len: u16 = labels.len().try_into()?;
                self.out.push_u16(labels_len);
                self.push_jump(*default, state, stack_height, None)?;
                // The label types are the same for the default as well all the other
                // labels.
                for label_idx in labels {
                    self.push_jump(*label_idx, state, stack_height, None)?;
                }
            }
            OpCode::Return => {
                // The interpreter will know that return means terminate execution of the
                // function and from the result type of the function it will be
                // clear whether anything needs to be returned.
                if self.return_type.is_some() {
                    let top = self.providers_stack.pop().context("Cannot return a value")?;
                    if top != Provider::Local(0) {
                        self.out.push(InternalOpcode::Copy);
                        self.push_loc(top);
                        self.push_loc(Provider::Local(0));
                    }
                }
                self.out.push(Return)
            }
            &OpCode::Call(idx) => {
                self.out.push(Call);
                self.out.push_u32(idx);
                let f = ctx.get_func(idx)?;
                // The interpreter knows the number of arguments already. No need to record.
                // Note that arguments are from **last** to first.
                for _ in &f.parameters {
                    self.push_consume()?;
                }
                // Return value, if it exists. The interpreter knows the return type
                // already.
                if f.result.is_some() {
                    self.push_provide();
                }
            }
            OpCode::CallImmediate(cost) => {
                self.out.push(CallImmediate);
                self.out.push_u32(*cost);
                // TODO
            }
            &OpCode::CallIndirect(idx) => {
                self.out.push(CallIndirect);
                self.out.push_u32(idx);
                self.push_consume()?;
                let f = ctx.get_type(idx)?;
                // The interpreter knows the number of arguments already. No need to record.
                // Note that arguments are from **last** to first.
                for _ in &f.parameters {
                    let provider = self
                        .providers_stack
                        .pop()
                        .context("Insufficient number of arguments for function.")?;
                    self.push_loc(provider);
                }
                // The interpreter knows the return type already.
                if f.result.is_some() {
                    self.push_provide();
                }
            }
            OpCode::Nop => {
                // do nothing, we don't need an opcode for that since we don't
                // care about alignment.
            }
            OpCode::Unreachable => {
                self.out.push(Unreachable);
            }
            OpCode::Drop => {
                let result =
                    self.providers_stack.pop().context("Drop requires a non-empty stack.")?;
                self.dynamic_locations.reuse(result);
            }
            OpCode::Select => {
                self.push_ternary(Select)?;
            }
            OpCode::LocalGet(idx) => {
                // the as i32 is safe because idx < NUM_ALLOWED_LOCALS <= 2^15
                self.providers_stack.push(Provider::Local((*idx) as i32));
                // No instruction
            }
            OpCode::LocalSet(idx) | OpCode::LocalTee(idx) => {
                // If the last instruction produced something just remove the indirection and
                // write directly to the local
                //
                // If the value of this particular local is somewhere on the providers_stack we
                // need to preserve that value.
                // - make a new dynamic value beyond all possible values. (the "preserve" space)
                // - copy from local to that value
                // TODO: This should be more efficiently tracked via a Set.
                let mut reserve = None;
                for provide_slot in &mut self.providers_stack {
                    if let Provider::Local(l) = *provide_slot {
                        if Ok(*idx) == u32::try_from(l) {
                            let reserve = match reserve {
                                Some(r) => r,
                                None => {
                                    // TODO: Do we need a separate preserve stack?
                                    let result = Provider::Dynamic(self.dynamic_locations.get());
                                    reserve = Some(result);
                                    result
                                }
                            };
                            // When an operation actually reads this value it will read it from the
                            // reserve slot.
                            *provide_slot = reserve;
                        }
                    }
                }
                let idx = (*idx).try_into()?; // This should never overflow since we have a very low bound on locals. But we
                                              // are just playing it safe.
                if let Some(reserve) = reserve {
                    self.out.push(Copy);
                    self.out.push_i32(idx); // from
                    self.push_loc(reserve); // to
                }
                if matches!(opcode, OpCode::LocalSet(..)) {
                    match last_provide {
                        // if we had to copy to a reserve location then it is not
                        // possible to short circuit the instruction.
                        // We need to insert an additional copy instruction.
                        Some(back_loc) if reserve.is_none() => {
                            // instead of inserting LocalSet, just tell the previous
                            // instruction to copy the value directly into the local.
                            self.out.back_patch(back_loc, idx as u32)?;

                            // And clear the provider from the stack
                            let operand = self
                                .providers_stack
                                .pop()
                                .with_context(|| format!("Missing operand for push_consume"))?;
                            self.dynamic_locations.reuse(operand);
                        }
                        _ => {
                            self.out.push(LocalSet);
                            let _operand = self.push_consume()?; // value first.
                            self.out.push_i32(idx); //target second
                        }
                    }
                } else {
                    match last_provide {
                        // if we had to copy to a reserve location then it is not
                        // possible to short circuit the instruction.
                        // We need to insert an additional copy instruction.
                        Some(back_loc) if reserve.is_none() => {
                            // instead of inserting LocalSet, just tell the previous
                            // instruction to copy the value directly into the local.
                            self.out.back_patch(back_loc, idx as u32)?;

                            // And clear the provider from the stack
                            let operand = self
                                .providers_stack
                                .pop()
                                .with_context(|| format!("Missing operand for push_consume"))?;
                            self.dynamic_locations.reuse(operand);
                            self.providers_stack.push(Provider::Local(idx));
                        }
                        _ => {
                            self.out.push(LocalSet);
                            let operand =
                                self.providers_stack.last().context("Expect a value on the stack")?;
                            self.push_loc(*operand);
                            self.out.push_i32(idx); //target second
                        }
                    }
                }
            }
            OpCode::GlobalGet(idx) => {
                self.out.push(GlobalGet);
                // the as u16 is safe because idx < MAX_NUM_GLOBALS <= 2^16
                self.out.push_u16(*idx as u16);
                self.push_provide();
            }
            OpCode::GlobalSet(idx) => {
                self.out.push(GlobalSet);
                // the as u16 is safe because idx < MAX_NUM_GLOBALS <= 2^16
                self.out.push_u16(*idx as u16);
                self.push_consume()?;
            }
            OpCode::I32Load(memarg) => {
                self.push_mem_load(I32Load, memarg)?;
            }
            OpCode::I64Load(memarg) => {
                self.push_mem_load(I64Load, memarg)?;
            }
            OpCode::I32Load8S(memarg) => {
                self.push_mem_load(I32Load8S, memarg)?;
            }
            OpCode::I32Load8U(memarg) => {
                self.push_mem_load(I32Load8U, memarg)?;
            }
            OpCode::I32Load16S(memarg) => {
                self.push_mem_load(I32Load16S, memarg)?;
            }
            OpCode::I32Load16U(memarg) => {
                self.push_mem_load(I32Load16U, memarg)?;
            }
            OpCode::I64Load8S(memarg) => {
                self.push_mem_load(I64Load8S, memarg)?;
            }
            OpCode::I64Load8U(memarg) => {
                self.push_mem_load(I64Load8U, memarg)?;
            }
            OpCode::I64Load16S(memarg) => {
                self.push_mem_load(I64Load16S, memarg)?;
            }
            OpCode::I64Load16U(memarg) => {
                self.push_mem_load(I64Load16U, memarg)?;
            }
            OpCode::I64Load32S(memarg) => {
                self.push_mem_load(I64Load32S, memarg)?;
            }
            OpCode::I64Load32U(memarg) => {
                self.push_mem_load(I64Load32U, memarg)?;
            }
            OpCode::I32Store(memarg) => {
                self.push_mem_store(I32Store, memarg)?;
            }
            OpCode::I64Store(memarg) => {
                self.push_mem_store(I64Store, memarg)?;
            }
            OpCode::I32Store8(memarg) => {
                self.push_mem_store(I32Store8, memarg)?;
            }
            OpCode::I32Store16(memarg) => {
                self.push_mem_store(I32Store16, memarg)?;
            }
            OpCode::I64Store8(memarg) => {
                self.push_mem_store(I64Store8, memarg)?;
            }
            OpCode::I64Store16(memarg) => {
                self.push_mem_store(I64Store16, memarg)?;
            }
            OpCode::I64Store32(memarg) => {
                self.push_mem_store(I64Store32, memarg)?;
            }
            OpCode::MemorySize => {
                self.out.push(MemorySize);
                self.push_provide();
            }
            OpCode::MemoryGrow => self.push_unary(MemoryGrow)?,
            OpCode::I32Const(c) => {
                let next = -i32::try_from(self.constants.len())? - 1;
                let idx = self.constants.entry((*c) as i64).or_insert(next);
                // Do not emit any instructions.
                self.providers_stack.push(Provider::Constant(*idx));
            }
            OpCode::I64Const(c) => {
                let next = -i32::try_from(self.constants.len())? - 1;
                let idx = self.constants.entry(*c).or_insert(next);
                // Do not emit any instructions.
                self.providers_stack.push(Provider::Constant(*idx));
            }
            OpCode::I32Eqz => {
                self.push_unary(I32Eqz)?;
            }
            OpCode::I32Eq => {
                self.push_binary(I32Eq)?;
            }
            OpCode::I32Ne => {
                self.push_binary(I32Ne)?;
            }
            OpCode::I32LtS => {
                self.push_binary(I32LtS)?;
            }
            OpCode::I32LtU => {
                self.push_binary(I32LtU)?;
            }
            OpCode::I32GtS => {
                self.push_binary(I32GtS)?;
            }
            OpCode::I32GtU => {
                self.push_binary(I32GtU)?;
            }
            OpCode::I32LeS => {
                self.push_binary(I32LeS)?;
            }
            OpCode::I32LeU => {
                self.push_binary(I32LeU)?;
            }
            OpCode::I32GeS => {
                self.push_binary(I32GeS)?;
            }
            OpCode::I32GeU => {
                self.push_binary(I32GeU)?;
            }
            OpCode::I64Eqz => {
                self.push_unary(I64Eqz)?;
            }
            OpCode::I64Eq => {
                self.push_binary(I64Eq)?;
            }
            OpCode::I64Ne => {
                self.push_binary(I64Ne)?;
            }
            OpCode::I64LtS => {
                self.push_binary(I64LtS)?;
            }
            OpCode::I64LtU => {
                self.push_binary(I64LtU)?;
            }
            OpCode::I64GtS => {
                self.push_binary(I64GtS)?;
            }
            OpCode::I64GtU => {
                self.push_binary(I64GtU)?;
            }
            OpCode::I64LeS => {
                self.push_binary(I64LeS)?;
            }
            OpCode::I64LeU => {
                self.push_binary(I64LeU)?;
            }
            OpCode::I64GeS => {
                self.push_binary(I64GeS)?;
            }
            OpCode::I64GeU => {
                self.push_binary(I64GeU)?;
            }
            OpCode::I32Clz => {
                self.push_unary(I32Clz)?;
            }
            OpCode::I32Ctz => {
                self.push_unary(I32Ctz)?;
            }
            OpCode::I32Popcnt => {
                self.push_unary(I32Popcnt)?;
            }
            OpCode::I32Add => {
                self.push_binary(I32Add)?;
            }
            OpCode::I32Sub => {
                self.push_binary(I32Sub)?;
            }
            OpCode::I32Mul => {
                self.push_binary(I32Mul)?;
            }
            OpCode::I32DivS => {
                self.push_binary(I32DivS)?;
            }
            OpCode::I32DivU => {
                self.push_binary(I32DivU)?;
            }
            OpCode::I32RemS => {
                self.push_binary(I32RemS)?;
            }
            OpCode::I32RemU => {
                self.push_binary(I32RemU)?;
            }
            OpCode::I32And => {
                self.push_binary(I32And)?;
            }
            OpCode::I32Or => {
                self.push_binary(I32Or)?;
            }
            OpCode::I32Xor => {
                self.push_binary(I32Xor)?;
            }
            OpCode::I32Shl => {
                self.push_binary(I32Shl)?;
            }
            OpCode::I32ShrS => {
                self.push_binary(I32ShrS)?;
            }
            OpCode::I32ShrU => {
                self.push_binary(I32ShrU)?;
            }
            OpCode::I32Rotl => {
                self.push_binary(I32Rotl)?;
            }
            OpCode::I32Rotr => {
                self.push_binary(I32Rotr)?;
            }
            OpCode::I64Clz => {
                self.push_unary(I64Clz)?;
            }
            OpCode::I64Ctz => {
                self.push_unary(I64Ctz)?;
            }
            OpCode::I64Popcnt => {
                self.push_unary(I64Popcnt)?;
            }
            OpCode::I64Add => {
                self.push_binary(I64Add)?;
            }
            OpCode::I64Sub => {
                self.push_binary(I64Sub)?;
            }
            OpCode::I64Mul => {
                self.push_binary(I64Mul)?;
            }
            OpCode::I64DivS => {
                self.push_binary(I64DivS)?;
            }
            OpCode::I64DivU => {
                self.push_binary(I64DivU)?;
            }
            OpCode::I64RemS => {
                self.push_binary(I64RemS)?;
            }
            OpCode::I64RemU => {
                self.push_binary(I64RemU)?;
            }
            OpCode::I64And => {
                self.push_binary(I64And)?;
            }
            OpCode::I64Or => {
                self.push_binary(I64Or)?;
            }
            OpCode::I64Xor => {
                self.push_binary(I64Xor)?;
            }
            OpCode::I64Shl => {
                self.push_binary(I64Shl)?;
            }
            OpCode::I64ShrS => {
                self.push_binary(I64ShrS)?;
            }
            OpCode::I64ShrU => {
                self.push_binary(I64ShrU)?;
            }
            OpCode::I64Rotl => {
                self.push_binary(I64Rotl)?;
            }
            OpCode::I64Rotr => {
                self.push_binary(I64Rotr)?;
            }
            OpCode::I32WrapI64 => {
                self.push_unary(I32WrapI64)?;
            }
            OpCode::I64ExtendI32S => {
                self.push_unary(I64ExtendI32S)?;
            }
            OpCode::I64ExtendI32U => {
                self.push_unary(I64ExtendI32U)?;
            }
            OpCode::I32Extend8S => {
                self.push_unary(I32Extend8S)?;
            }
            OpCode::I32Extend16S => {
                self.push_unary(I32Extend16S)?;
            }
            OpCode::I64Extend8S => {
                self.push_unary(I64Extend8S)?;
            }
            OpCode::I64Extend16S => {
                self.push_unary(I64Extend16S)?;
            }
            OpCode::I64Extend32S => {
                self.push_unary(I64Extend32S)?;
            }
        }
        Ok(())
    }

    fn finish(self, _state: &ValidationState) -> CompileResult<Self::Outcome> {
        ensure!(self.backpatch.stack.is_empty(), "There are still jumps to backpatch.");
        let mut constants = vec![0; self.constants.len()];
        for (value, place) in self.constants {
            *constants
                .get_mut(usize::try_from(-(place + 1))?)
                .context("Invariant violation. All locations are meant to be consecutive.")? =
                value;
        }
        Ok((self.out, self.dynamic_locations.next_location, constants))
    }
}

struct ModuleContext<'a> {
    module: &'a Module,
    locals: &'a [LocalsRange],
    code:   &'a Code,
}

impl<'a> HasValidationContext for ModuleContext<'a> {
    fn get_local(&self, idx: u32) -> CompileResult<ValueType> {
        let res = self.locals.binary_search_by(|locals| {
            if locals.end <= idx {
                std::cmp::Ordering::Less
            } else if idx < locals.start {
                std::cmp::Ordering::Greater
            } else {
                std::cmp::Ordering::Equal
            }
        });
        match res {
            Ok(idx) => Ok(self.locals[idx].ty),
            Err(_) => bail!("Local index out of range."),
        }
    }

    fn get_global(&self, idx: crate::types::GlobalIndex) -> CompileResult<(ValueType, bool)> {
        match self.module.global.globals.get(idx as usize) {
            Some(g) => Ok((ValueType::from(g), g.mutable)),
            None => bail!("Attempting to access non-existing global."),
        }
    }

    fn memory_exists(&self) -> bool { self.module.memory.memory_type.is_some() }

    fn table_exists(&self) -> bool { self.module.table.table_type.is_some() }

    fn get_func(&self, idx: FuncIndex) -> CompileResult<&std::rc::Rc<FunctionType>> {
        if (idx as usize) < self.module.import.imports.len() {
            match self.module.import.imports[idx as usize].description {
                ImportDescription::Func {
                    type_idx,
                } => self
                    .module
                    .ty
                    .get(type_idx)
                    .ok_or_else(|| anyhow!("Attempting to get type that does not exist")),
            }
        } else {
            self.module
                .code
                .impls
                .get(idx as usize - self.module.import.imports.len())
                .map(|c| &c.ty)
                .ok_or_else(|| anyhow!("Attempting to get type of function that does not exist."))
        }
    }

    fn get_type(&self, idx: TypeIndex) -> CompileResult<&std::rc::Rc<FunctionType>> {
        self.module
            .ty
            .types
            .get(idx as usize)
            .ok_or_else(|| anyhow!("Attempting to get non-existing type."))
    }

    fn return_type(&self) -> BlockType { BlockType::from(self.code.ty.result) }
}

/// Compile a module into an artifact, failing if there are problems.
/// Problems should not arise if the module is well-formed, and all the imports
/// are supported by the `I` type.
impl Module {
    pub fn compile<I: TryFromImport>(self) -> CompileResult<Artifact<I, CompiledFunction>> {
        let mut code_out = Vec::with_capacity(self.code.impls.len());

        for code in self.code.impls.iter() {
            let mut ranges = Vec::with_capacity(code.ty.parameters.len() + code.locals.len());
            let mut locals = Vec::with_capacity(code.ty.parameters.len() + code.locals.len());
            let mut start = 0;
            for &param in code.ty.parameters.iter() {
                let end = start + 1;
                ranges.push(LocalsRange {
                    start,
                    end,
                    ty: param,
                });
                start = end;
            }
            for &local in code.locals.iter() {
                locals.push(ArtifactLocal::try_from(local)?);
                let end = start + local.multiplicity;
                ranges.push(LocalsRange {
                    start,
                    end,
                    ty: local.ty,
                });
                start = end;
            }

            let context = ModuleContext {
                module: &self,
                locals: &ranges,
                code,
            };

            let (mut exec_code, num_registers, constants) = validate(
                &context,
                code.expr.instrs.iter().map(Result::Ok),
                BackPatch::new(start, code.ty.result),
            )?;
            // We add a return instruction at the end so we have an easier time in the
            // interpreter since there is no implicit return.

            // No need to insert an additional Copy here. The `End` block will insert it if
            // needed.
            exec_code.push(InternalOpcode::Return);

            let num_params: u32 = code.ty.parameters.len().try_into()?;

            let result = CompiledFunction {
                type_idx: code.ty_idx,
                params: code.ty.parameters.clone(),
                num_locals: start - num_params,
                locals,
                return_type: BlockType::from(code.ty.result),
                num_registers: num_registers.try_into()?,
                constants,
                code: exec_code,
            };
            code_out.push(result)
        }

        let ty = self.ty.types.into_iter().map(|x| (*x).clone()).collect::<Vec<FunctionType>>();
        let table = {
            if let Some(tt) = self.table.table_type {
                let mut functions = vec![None; tt.limits.min as usize];
                for init in self.element.elements.iter() {
                    // validation has already ensured that inits are within bounds.
                    for (place, value) in
                        functions[init.offset as usize..].iter_mut().zip(init.inits.iter())
                    {
                        *place = Some(*value)
                    }
                }
                InstantiatedTable {
                    functions,
                }
            } else {
                InstantiatedTable {
                    functions: Vec::new(),
                }
            }
        };
        let memory = {
            if let Some(mt) = self.memory.memory_type {
                Some(ArtifactMemory {
                    init_size: mt.limits.min,
                    max_size:  mt
                        .limits
                        .max
                        .map(|x| std::cmp::min(x, MAX_NUM_PAGES))
                        .unwrap_or(MAX_NUM_PAGES),
                    init:      self
                        .data
                        .sections
                        .into_iter()
                        .map(ArtifactData::from)
                        .collect::<Vec<_>>(),
                })
            } else {
                None
            }
        };
        let global = InstantiatedGlobals {
            inits: self.global.globals.iter().map(|x| x.init).collect::<Vec<_>>(),
        };
        let export = self
            .export
            .exports
            .into_iter()
            .filter_map(|export| {
                if let ExportDescription::Func {
                    index,
                } = export.description
                {
                    Some((export.name, index))
                } else {
                    None
                }
            })
            .collect::<BTreeMap<_, _>>();
        let imports = self
            .import
            .imports
            .into_iter()
            .map(|i| I::try_from_import(&ty, i))
            .collect::<CompileResult<_>>()?;
        Ok(Artifact {
            imports,
            ty,
            table,
            memory,
            global,
            export,
            code: code_out,
        })
    }
}
