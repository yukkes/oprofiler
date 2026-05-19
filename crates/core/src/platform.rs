use std::process::{Command, Stdio};

#[cfg(target_os = "windows")]
use std::os::windows::process::CommandExt;

pub fn hidden_command(program: &str) -> Command {
    let mut cmd = Command::new(program);
    #[cfg(target_os = "windows")]
    cmd.creation_flags(0x08000000);
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    cmd
}

pub fn native_library_file_name() -> &'static str {
    #[cfg(target_os = "windows")]
    {
        "jvmti_agent_rust.dll"
    }
    #[cfg(target_os = "macos")]
    {
        "libjvmti_agent_rust.dylib"
    }
    #[cfg(all(not(target_os = "windows"), not(target_os = "macos")))]
    {
        "libjvmti_agent_rust.so"
    }
}
