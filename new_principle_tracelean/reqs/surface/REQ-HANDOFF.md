# REQ-HANDOFF: Handing a file to another program

- status: approved
- decomposition: complete
- refines: ARCH-NO-DRIVING, ARCH-CORE-SHELL

Some work is done better elsewhere: a proof's goals are read in VS Code's
infoview, the report is read in a browser, and a folder is chosen in the
system's own picker. The editor starts the program the way a shell would, gives
it nothing on its input and keeps no channel to it, so a handed-off program is
never driven (`ARCH-NO-DRIVING`). VS Code and the browser are let go at once;
the picker is waited for away from the keys, and the one line it prints is the
folder. What to start, and what the picker's answer means, are decisions from
data and are modelled (`TraceLean.Handoff`); starting the program is the only
effect.

## Clauses

- **file_to_vscode**: When asked to open a file in VS Code, the system shall start VS Code on the file's absolute path at the line and column of the cursor counted from one, and at the first line and column when the cursor is in another buffer.
- **report_to_browser**: When the report is written from the editor, the system shall open its page in the system's default browser, and when it is written from the command line, it shall not.
- **started_not_driven**: The system shall start a handed-off program with no input and without making a key wait for it, and shall write nothing to it.
- **folder_picked**: When asked to open a project, the system shall show the folder picker of the system's own desktop.
- **choice_opened**: When the folder picker answers, the system shall open the folder it chose, and shall open nothing when it was closed without a choice.
- **failure_named**: If a handed-off program cannot be started, then the system shall name the program and why on the status line.
