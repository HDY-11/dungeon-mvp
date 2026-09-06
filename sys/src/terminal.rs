//! 终端状态管理。

use crossterm::ExecutableCommand;
use crossterm::terminal::{
    EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode,
};
use std::io::{self, stdout};

pub fn enter_raw_mode() -> io::Result<()> {
    enable_raw_mode()
}

pub fn leave_raw_mode() -> io::Result<()> {
    disable_raw_mode()
}

pub fn enter_alternate_screen() -> io::Result<()> {
    stdout().execute(EnterAlternateScreen)?;
    Ok(())
}

pub fn leave_alternate_screen() -> io::Result<()> {
    stdout().execute(LeaveAlternateScreen)?;
    Ok(())
}

pub fn show_cursor() -> io::Result<()> {
    use crossterm::cursor::Show;
    stdout().execute(Show)?;
    Ok(())
}
