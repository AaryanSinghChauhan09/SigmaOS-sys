use sigmaos_sys::*;

#[test]
fn sys_table_smoke() {
    let mut t = SyscallTable::new();
    t.register(SyscallEntry {
        nr: 60, // exit
        name: "exit",
        required: None,
        handler: Box::new(FnHandler(|a: &SyscallArgs| Ok(a.arg(0)))),
    }).unwrap();
    let args = SyscallArgs::new(60).with_arg(0, 0);
    assert_eq!(t.dispatch(&args, &[]).unwrap(), 0);
}
