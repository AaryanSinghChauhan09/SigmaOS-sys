//! SigmaOS-sys — shared syscall/ABI abstraction layer.
//!
//! The single mechanism through which userland reaches kernel services:
//! - [`SyscallHandler`]: trait every kernel subsystem implements
//! - [`SyscallTable`]: number -> handler registry (dispatch, like Linux's
//!   `sys_call_table`)
//! - [`AbiError`]: kernel-side error type mapped to userspace errno
//! - [`Capability`]: least-privilege permission flags checked at dispatch
//!
//! Policy (which capabilities a process holds) is decided above this
//! layer; this crate is pure mechanism — stable ABI, no hidden state.

use std::collections::BTreeMap;
use std::fmt;

/// Kernel-side syscall error; converted to userspace errno at the boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AbiError {
    /// Maps to EPERM (1).
    PermissionDenied,
    /// Maps to ENOENT (2).
    NotFound,
    /// Maps to EINVAL (22).
    InvalidArgument,
    /// Maps to ENOSYS (38).
    NotImplemented,
    /// Maps to EAGAIN (11).
    WouldBlock,
    /// Maps to EFAULT (14).
    BadAddress,
    /// Maps to EIO (5).
    Io,
    /// Maps to ENOMEM (12).
    OutOfMemory,
    /// Custom errno value for subsystem-specific errors.
    Custom(i32),
}

impl AbiError {
    /// POSIX errno number (x86_64 asm-generic values).
    pub fn errno(&self) -> i32 {
        match self {
            AbiError::PermissionDenied => 1,
            AbiError::NotFound => 2,
            AbiError::Io => 5,
            AbiError::WouldBlock => 11,
            AbiError::OutOfMemory => 12,
            AbiError::BadAddress => 14,
            AbiError::InvalidArgument => 22,
            AbiError::NotImplemented => 38,
            AbiError::Custom(n) => *n,
        }
    }
}

impl fmt::Display for AbiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "abi error {}", self.errno())
    }
}
impl std::error::Error for AbiError {}

/// Syscall argument block — raw registers, like the kernel entry frame.
#[derive(Debug, Clone, Default)]
pub struct SyscallArgs {
    pub nr: u64,
    pub arg0: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
}

impl SyscallArgs {
    pub fn new(nr: u64) -> Self {
        Self { nr, ..Default::default() }
    }
    pub fn arg(&self, i: usize) -> u64 {
        match i {
            0 => self.arg0,
            1 => self.arg1,
            2 => self.arg2,
            3 => self.arg3,
            4 => self.arg4,
            5 => self.arg5,
            _ => 0,
        }
    }
    pub fn with_arg(mut self, i: usize, v: u64) -> Self {
        match i {
            0 => self.arg0 = v,
            1 => self.arg1 = v,
            2 => self.arg2 = v,
            3 => self.arg3 = v,
            4 => self.arg4 = v,
            5 => self.arg5 = v,
            _ => {}
        }
        self
    }
}

/// One syscall implementation — the `sys_*` function analogue.
/// Handlers are pure mechanism: they get raw args and return a raw value
/// or an [`AbiError`] that becomes the userspace errno.
pub type SyscallResult = Result<u64, AbiError>;
pub trait SyscallHandler: Send + Sync {
    fn handle(&self, args: &SyscallArgs) -> SyscallResult;
}

/// Capability flags — least privilege, checked before dispatch.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Capability {
    /// CAP_SYS_ADMIN analogue — mount, module load, etc.
    SysAdmin = 1 << 0,
    /// CAP_NET_ADMIN analogue — interface/route mutation.
    NetAdmin = 1 << 1,
    /// CAP_SYS_PTRACE analogue — process inspection/control.
    SysPtrace = 1 << 2,
    /// CAP_DAC_OVERRIDE analogue — bypass file permission checks.
    DacOverride = 1 << 3,
    /// CAP_SETUID analogue — credential changes.
    SetUid = 1 << 4,
}

/// A registered syscall: number, required capability, handler.
pub struct SyscallEntry {
    pub nr: u64,
    pub name: &'static str,
    pub required: Option<Capability>,
    pub handler: Box<dyn SyscallHandler>,
}

/// Errors from table construction/dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TableError {
    DuplicateNumber(u64),
    UnknownSyscall(u64),
    MissingCapability { nr: u64, needed: Capability },
}

impl fmt::Display for TableError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TableError::DuplicateNumber(n) => write!(f, "duplicate syscall number {n}"),
            TableError::UnknownSyscall(n) => write!(f, "unknown syscall {n}"),
            TableError::MissingCapability { nr, needed } => {
                write!(f, "syscall {nr} requires capability {needed:?}")
            }
        }
    }
}
impl std::error::Error for TableError {}

/// `sys_call_table` analogue.
#[derive(Default)]
pub struct SyscallTable {
    entries: BTreeMap<u64, SyscallEntry>,
}

impl SyscallTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn register(&mut self, entry: SyscallEntry) -> Result<(), TableError> {
        if self.entries.contains_key(&entry.nr) {
            return Err(TableError::DuplicateNumber(entry.nr));
        }
        self.entries.insert(entry.nr, entry);
        Ok(())
    }

    /// Dispatch a syscall for a caller holding `caps`.
    ///
    /// Returns the raw syscall-ABI result: `Ok(value)` on success,
    /// `Ok(-errno)` on handler error (Linux convention), and
    /// `Err(TableError)` for unknown numbers / missing capabilities.
    pub fn dispatch(&self, args: &SyscallArgs, caps: &[Capability]) -> Result<i64, TableError> {
        let entry = self
            .entries
            .get(&args.nr)
            .ok_or(TableError::UnknownSyscall(args.nr))?;
        if let Some(need) = entry.required {
            if !caps.contains(&need) {
                return Err(TableError::MissingCapability { nr: args.nr, needed: need });
            }
        }
        match entry.handler.handle(args) {
            Ok(v) => Ok(v as i64),
            Err(e) => Ok(-(e.errno() as i64)),
        }
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn contains(&self, nr: u64) -> bool {
        self.entries.contains_key(&nr)
    }
}

/// Convenience: boxed closure handler.
pub struct FnHandler<F>(pub F);
impl<F> SyscallHandler for FnHandler<F>
where
    F: Fn(&SyscallArgs) -> SyscallResult + Send + Sync,
{
    fn handle(&self, args: &SyscallArgs) -> SyscallResult {
        (self.0)(args)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table_with_getpid() -> SyscallTable {
        let mut t = SyscallTable::new();
        t.register(SyscallEntry {
            nr: 39, // getpid on x86_64
            name: "getpid",
            required: None,
            handler: Box::new(FnHandler(|_: &SyscallArgs| Ok(4242))),
        })
        .unwrap();
        t
    }

    #[test]
    fn dispatch_returns_handler_value() {
        let t = table_with_getpid();
        let args = SyscallArgs::new(39);
        assert_eq!(t.dispatch(&args, &[]).unwrap(), 4242);
    }

    #[test]
    fn dispatch_raw_returns_negative_errno() {
        let mut t = SyscallTable::new();
        t.register(SyscallEntry {
            nr: 1,
            name: "write",
            required: None,
            handler: Box::new(FnHandler(|_: &SyscallArgs| Err(AbiError::BadAddress))),
        })
        .unwrap();
        let args = SyscallArgs::new(1);
        assert_eq!(t.dispatch(&args, &[]).unwrap(), -14);
    }

    #[test]
    fn unknown_syscall_fails() {
        let t = SyscallTable::new();
        assert!(matches!(
            t.dispatch(&SyscallArgs::new(9999), &[]),
            Err(TableError::UnknownSyscall(9999))
        ));
    }

    #[test]
    fn capability_gate_enforced() {
        let mut t = SyscallTable::new();
        t.register(SyscallEntry {
            nr: 165, // mount
            name: "mount",
            required: Some(Capability::SysAdmin),
            handler: Box::new(FnHandler(|_: &SyscallArgs| Ok(0))),
        })
        .unwrap();
        let args = SyscallArgs::new(165);
        assert!(matches!(
            t.dispatch(&args, &[]),
            Err(TableError::MissingCapability { .. })
        ));
        assert_eq!(t.dispatch(&args, &[Capability::SysAdmin]).unwrap(), 0);
    }

    #[test]
    fn duplicate_registration_rejected() {
        let mut t = SyscallTable::new();
        let e = || SyscallEntry {
            nr: 1, name: "x", required: None,
            handler: Box::new(FnHandler(|_: &SyscallArgs| Ok(0))),
        };
        t.register(e()).unwrap();
        assert!(matches!(t.register(e()), Err(TableError::DuplicateNumber(1))));
    }

    #[test]
    fn args_accessor_roundtrip() {
        let a = SyscallArgs::new(9).with_arg(0, 0x1000).with_arg(2, 4096);
        assert_eq!(a.arg(0), 0x1000);
        assert_eq!(a.arg(2), 4096);
        assert_eq!(a.arg(5), 0);
    }
}
