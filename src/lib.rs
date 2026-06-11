pub mod asio;
pub mod com;
pub mod driver;
pub mod gui;

#[macro_export]
macro_rules! rlog {
    ($($arg:tt)*) => {{
        use std::io::Write;
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open("/home/sfinxv/.var/app/com.usebottles.bottles/data/bottles/bottles/FL-Studio__110/drive_c/rwasio.log")
        {
            let _ = writeln!(f, $($arg)*);
        }
    }};
}
