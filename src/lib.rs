pub mod asio;
pub mod com;
pub mod driver;

#[unsafe(no_mangle)]
pub extern "C" fn HelloFromLinux() -> i32 {
    let pid = getpid();
    let msg = b"[hello.dll] Wine loaded me, but I am Linux!\n";
    write_stderr(msg);
    pid
}

fn getpid() -> i32 {
    let pid: i64;
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") 39i64 => pid,
            options(nostack)
        );
    }
    pid as i32
}

fn write_stderr(msg: &[u8]) {
    unsafe {
        std::arch::asm!(
            "syscall",
            inlateout("rax") 1i64 => _,
            in("rdi") 2i64,
            in("rsi") msg.as_ptr(),
            in("rdx") msg.len(),
            options(nostack)
        );
    }
}
