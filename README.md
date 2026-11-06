# SigmaOS-sys

Shared syscall/ABI abstraction layer — the single mechanism by which
userland reaches SigmaOS kernel services.

- `SyscallTable`: syscall-number → handler registry (`sys_call_table` analogue)
- `SyscallHandler`: trait implemented by kernel subsystems
- `AbiError`: kernel errors mapped to POSIX errno; dispatch returns
  `Ok(-errno)` per the Linux syscall ABI convention
- `Capability`: least-privilege flags enforced at dispatch time

Pure mechanism — capability policy lives in the process layer above.

`cargo test` — deterministic unit + integration tests.
