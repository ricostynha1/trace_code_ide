namespace TraceLean.Handoff

/-- @models REQ-HANDOFF.file_to_vscode -/
def inVscode (os : String) (file : String) (line : Nat) (column : Nat) : List String :=
  let goto := ["--goto", file ++ ":" ++ toString (line + 1) ++ ":" ++ toString (column + 1)]
  if os == "windows" then ["cmd", "/c", "code"] ++ goto else "code" :: goto

/-- @models REQ-HANDOFF.report_to_browser -/
def inBrowser (os : String) (page : String) : List String :=
  if os == "macos" then ["open", page]
  else if os == "windows" then ["cmd", "/c", "start", "", page]
  else ["xdg-open", page]

/-- @models REQ-HANDOFF.folder_picked -/
def folderPicker (os : String) (desktop : String) (start : String) : List String :=
  if os == "macos" then ["osascript", "-e", "POSIX path of (choose folder)"]
  else if os == "windows" then
    ["powershell", "-NoProfile", "-Command",
     "Add-Type -AssemblyName System.Windows.Forms; $d = New-Object System.Windows.Forms.FolderBrowserDialog; $null = $d.ShowDialog(); $d.SelectedPath"]
  else if (desktop.splitOn "KDE").length > 1 then ["kdialog", "--getexistingdirectory", start]
  else ["zenity", "--file-selection", "--directory", "--filename=" ++ start ++ "/"]

/-- @models REQ-HANDOFF.choice_opened -/
def pickedFolder (said : String) : Option String :=
  let line := (said.splitOn "\n").headD ""
  let line := if line.endsWith "\r" then line.dropRight 1 else line
  if line == "" then none else some line

/-- @proves REQ-HANDOFF.file_to_vscode -/
theorem vscode_goes_to_the_cursor_counted_from_one (os file : String) (line column : Nat) :
    (inVscode os file line column).getLast? =
      some (file ++ ":" ++ toString (line + 1) ++ ":" ++ toString (column + 1)) := by
  unfold inVscode
  split <;> simp

/-- @proves REQ-HANDOFF.report_to_browser -/
theorem the_browser_is_given_the_page (os page : String) :
    (inBrowser os page).getLast? = some page := by
  unfold inBrowser
  split
  · simp
  · split <;> simp

end TraceLean.Handoff
