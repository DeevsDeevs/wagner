use base64::{Engine, engine::general_purpose::STANDARD};
use std::io::{Write, stdout};
use std::process::{Command, Stdio};

pub fn copy_to_clipboard(text: &str) -> Result<(), String> {
    if copy_osc52(text).is_ok() {
        return Ok(());
    }

    #[cfg(target_os = "macos")]
    {
        copy_with_command("pbcopy", &[], text)
    }

    #[cfg(target_os = "linux")]
    {
        if let Ok(()) = copy_with_command("wl-copy", &[], text) {
            return Ok(());
        }
        if let Ok(()) = copy_with_command("xclip", &["-selection", "clipboard"], text) {
            return Ok(());
        }
        if let Ok(()) = copy_with_command("xsel", &["--clipboard", "--input"], text) {
            return Ok(());
        }
        return Err("No clipboard tool found. Install wl-copy, xclip, or xsel.".to_string());
    }

    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err("Clipboard not supported on this platform".to_string())
    }
}

fn copy_osc52(text: &str) -> Result<(), String> {
    let encoded = STANDARD.encode(text);
    let osc52 = format!("\x1b]52;c;{}\x07", encoded);
    let mut out = stdout();
    out.write_all(osc52.as_bytes()).map_err(|e| e.to_string())?;
    out.flush().map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_with_command(cmd: &str, args: &[&str], text: &str) -> Result<(), String> {
    let mut child = Command::new(cmd)
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("{}: {}", cmd, e))?;

    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(text.as_bytes())
            .map_err(|e| format!("Failed to write to {}: {}", cmd, e))?;
    }

    let status = child.wait().map_err(|e| format!("{}: {}", cmd, e))?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("{} failed with status: {}", cmd, status))
    }
}
