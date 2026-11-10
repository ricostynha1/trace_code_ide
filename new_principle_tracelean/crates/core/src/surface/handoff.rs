/// @implements REQ-HANDOFF.file_to_vscode
/// @drt REQ-HANDOFF.file_to_vscode
pub fn in_vscode(os: String, file: String, line: usize, column: usize) -> Vec<String> {
    let goto = vec!["--goto".to_string(), format!("{file}:{}:{}", line + 1, column + 1)];
    let program: Vec<String> = if os == "windows" { vec!["cmd".into(), "/c".into(), "code".into()] } else { vec!["code".into()] };
    program.into_iter().chain(goto).collect()
}

/// @implements REQ-HANDOFF.report_to_browser
/// @drt REQ-HANDOFF.report_to_browser
pub fn in_browser(os: String, page: String) -> Vec<String> {
    match os.as_str() {
        "macos" => vec!["open".into(), page],
        "windows" => vec!["cmd".into(), "/c".into(), "start".into(), String::new(), page],
        _ => vec!["xdg-open".into(), page],
    }
}

/// @implements REQ-HANDOFF.folder_picked
/// @drt REQ-HANDOFF.folder_picked
pub fn folder_picker(os: String, desktop: String, start: String) -> Vec<String> {
    let words = |w: &[&str]| w.iter().map(|w| w.to_string()).collect::<Vec<_>>();
    match os.as_str() {
        "macos" => words(&["osascript", "-e", "POSIX path of (choose folder)"]),
        "windows" => words(&[
            "powershell",
            "-NoProfile",
            "-Command",
            "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $null = $d.ShowDialog(); $d.SelectedPath",
        ]),
        _ if desktop.contains("KDE") => vec!["kdialog".into(), "--getexistingdirectory".into(), start],
        _ => vec!["zenity".into(), "--file-selection".into(), "--directory".into(), format!("--filename={start}/")],
    }
}

/// @implements REQ-HANDOFF.choice_opened
/// @drt REQ-HANDOFF.choice_opened
pub fn picked_folder(said: String) -> Option<String> {
    let line = said.split('\n').next().unwrap_or_default();
    let line = line.strip_suffix('\r').unwrap_or(line);
    (!line.is_empty()).then(|| line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// @tests REQ-HANDOFF.file_to_vscode
    #[test]
    fn vscode_is_sent_to_the_cursor_counted_from_one() {
        assert_eq!(in_vscode("linux".into(), "/p/a.lean".into(), 0, 0), ["code", "--goto", "/p/a.lean:1:1"]);
        assert_eq!(in_vscode("windows".into(), "C:/a.lean".into(), 4, 2), ["cmd", "/c", "code", "--goto", "C:/a.lean:5:3"]);
    }

    /// @tests REQ-HANDOFF.folder_picked
    /// @tests REQ-HANDOFF.choice_opened
    #[test]
    fn the_picker_is_the_desktop_own_and_its_line_is_the_folder() {
        assert_eq!(folder_picker("linux".into(), "KDE".into(), "/p".into()), ["kdialog", "--getexistingdirectory", "/p"]);
        assert_eq!(folder_picker("linux".into(), "GNOME".into(), "/p".into())[0], "zenity");
        assert_eq!(folder_picker("macos".into(), String::new(), "/p".into())[0], "osascript");
        assert_eq!(picked_folder("/home/u/proj\n".into()), Some("/home/u/proj".into()));
        assert_eq!(picked_folder("C:\\p\r\n".into()), Some("C:\\p".into()));
        assert_eq!(picked_folder(String::new()), None);
    }

    /// @tests REQ-HANDOFF.report_to_browser
    #[test]
    fn the_page_goes_to_the_system_browser() {
        assert_eq!(in_browser("linux".into(), "/r/index.html".into()), ["xdg-open", "/r/index.html"]);
        assert_eq!(in_browser("macos".into(), "/r/index.html".into()), ["open", "/r/index.html"]);
        assert_eq!(in_browser("windows".into(), "C:/r.html".into()), ["cmd", "/c", "start", "", "C:/r.html"]);
    }
}
