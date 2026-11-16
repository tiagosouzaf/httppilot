//! Platform file pickers for Postman JSON documents.
//!
//! No WebView or added runtime dependency is needed. On Linux, invoke a
//! system-installed Zenity or KDialog native picker; users can also paste a
//! file path if neither desktop dialog service is installed.

use std::process::Command;

pub fn select_json_file(save: bool) -> Result<Option<String>, String> {
    #[cfg(target_os = "windows")]
    {
        let kind = if save {
            "System.Windows.Forms.SaveFileDialog"
        } else {
            "System.Windows.Forms.OpenFileDialog"
        };
        let script = format!(
            "Add-Type -AssemblyName System.Windows.Forms; \
             $dialog = New-Object {kind}; \
             $dialog.Filter = 'JSON files (*.json)|*.json|All files (*.*)|*.*'; \
             if ($dialog.ShowDialog() -eq [System.Windows.Forms.DialogResult]::OK) {{ \
                 [Console]::Write($dialog.FileName) \
             }}"
        );
        return command_result(Command::new("powershell.exe").args([
            "-NoProfile",
            "-NonInteractive",
            "-STA",
            "-Command",
            &script,
        ]));
    }

    #[cfg(target_os = "linux")]
    {
        let mut zenity = Command::new("zenity");
        zenity.args(["--file-selection", "--title=Postman collection JSON"]);
        zenity.arg("--file-filter=JSON files | *.json");
        if save {
            zenity.arg("--save");
        }

        match command_result(&mut zenity) {
            Ok(result) => return Ok(result),
            Err(error) if error.starts_with("dialog executable unavailable:") => {}
            Err(error) => return Err(error),
        }

        let mut kdialog = Command::new("kdialog");
        kdialog.args(if save {
            ["--getsavefilename", ".", "JSON files (*.json)"]
        } else {
            ["--getopenfilename", ".", "JSON files (*.json)"]
        });
        command_result(&mut kdialog)
    }

    #[cfg(not(any(target_os = "windows", target_os = "linux")))]
    {
        let _ = save;
        Err("File dialog is not yet supported on this platform".to_owned())
    }
}

#[cfg(any(target_os = "windows", target_os = "linux"))]
fn command_result(command: &mut Command) -> Result<Option<String>, String> {
    let output = command.output().map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            format!("dialog executable unavailable: {error}")
        } else {
            format!("could not open native file dialog: {error}")
        }
    })?;

    if !output.status.success() {
        // The user pressed Cancel or closed the dialog.
        return Ok(None);
    }

    let path = String::from_utf8(output.stdout)
        .map_err(|error| format!("non-UTF-8 file name from file dialog: {error}"))?;
    let path = path.trim();
    if path.is_empty() {
        Ok(None)
    } else {
        Ok(Some(path.to_owned()))
    }
}
