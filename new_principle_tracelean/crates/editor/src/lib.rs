//! The editor's state, and what an intent does to it.
//!
//! This is a shell: it reads the working tree and writes it back. Every
//! decision it could make instead belongs in the core, and the ones that are
//! here are the ones that need the disk.
//!
//! It is a crate of its own because *both* frontends drive it. A terminal and a
//! web view that each performed intents their own way would be two answers to
//! what the editor does, which is the same failure one representation exists to
//! prevent, one layer up. A frontend renders and sends keys; this decides.
//!
//! What it never does is build a buffer of its own. Every buffer on screen came
//! from `surface::produce`, which is what `REQ-SHOW.core_produces` asks.
//!
//! @implements REQ-SHOW.core_produces
//! @implements REQ-ACT.one_path

pub mod recall;
pub mod theme;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tracelean_core::evidence::Level;
use tracelean_core::history::command::{Command, Workspace};
use tracelean_core::history::tree::Tree;
use tracelean_core::observe::transcript::Event;
use tracelean_core::surface::act::{dispatch, Blocked, Focus, Intent, Move, Watch};
use tracelean_core::surface::keymap::{Binding, Keymap, Outcome};
use tracelean_core::surface::explorer;
use tracelean_core::surface::produce as make;
use tracelean_core::surface::requirement_view;
use tracelean_core::surface::screen::{
    arrange, destination, panes, place, workbench, Arrangement, Layout, Rect, Screen,
};
use tracelean_core::surface::view::{actions_at, Buffer, BufferKind};
use tracelean_core::trace::anchor::Lang;

/// Everything on screen, and everything behind it.
pub struct Editor {
    pub root: PathBuf,
    pub keymap: Keymap,
    /// The keymap mode the next key is read in.
    pub mode: String,
    /// What is opened, what is placed, and which pane has focus.
    ///
    /// The single store of the buffers this session holds: `buffer()` reads the
    /// focused pane's out of it rather than keeping a second copy beside it,
    /// because two copies of one buffer is two answers to what it contains.
    ///
    /// @implements REQ-SCREEN.screen_is_a_value
    pub screen: Screen,
    /// The region a frontend last drew into.
    ///
    /// Moving the focus is a question about geometry and the screen holds none:
    /// the same layout is arranged differently in a narrow terminal and a wide
    /// window. A frontend says how big it is; the editor does not guess.
    pub region: Rect,
    /// The cursor, as a character offset into the focused buffer's text.
    pub offset: usize,
    /// Where a selection began, when there is one: it runs from here to the
    /// cursor. Kept here rather than in a frontend, so it survives scrolling
    /// and reaches past what is on screen.
    pub anchor: Option<usize>,
    /// The status line, which is a buffer like everything else.
    pub status: Buffer,
    /// The menu for the mode the editor is in, while it is in one.
    ///
    /// Beside the buffer rather than instead of it. A leader menu is something
    /// offered *about* what is on screen, and replacing the screen with it would
    /// move the cursor off the thing the next key is about — `Space f o` would
    /// open the menu row the cursor landed on rather than the file.
    ///
    /// @implements REQ-SHOW.menu_from_keymap
    pub menu: Option<Buffer>,
    /// The first line shown in the focused pane. A frontend draws everything it
    /// is given, so what it is given is the window starting here.
    pub top: usize,
    /// Where the cursor and the first shown line were in each pane the focus
    /// has left, so that coming back to a pane comes back to where you were.
    ///
    /// The focused pane's are `offset` and `top`; these are every other pane's.
    parked: BTreeMap<String, (usize, usize)>,
    /// The same, per buffer a pane stopped showing: going back to a tab goes
    /// back to where you were in it.
    places: BTreeMap<String, (usize, usize)>,
    /// Where the document was before each jump (another file, a definition,
    /// a line), newest last, and where going back left from: Alt+Left and
    /// Alt+Right.
    back: Vec<(String, usize)>,
    forward: Vec<(String, usize)>,
    /// Every change, so that undo is never a special case.
    pub tree: Tree,
    /// The workspace at a point of the history, kept so that asking for it
    /// does not replay the whole history from the base each time — which, on
    /// every key, made typing slower the longer a session ran.
    held_state: std::cell::RefCell<Option<(Option<tracelean_core::history::tree::NodeId>, Workspace)>>,
    /// Each drawn file's chips, with the text they were counted from.
    chips_held: std::cell::RefCell<BTreeMap<String, (String, Vec<tracelean_core::surface::chips::Chip>)>>,
    /// The workspace as it was last written out, for the review buffer.
    saved: Workspace,
    /// Changes seen in the working tree that the editor has not taken in.
    ///
    /// This is how work done outside the editor arrives. Nothing here starts
    /// it: an agent runs where a person started it, and the editor reads the
    /// tree it left behind.
    ///
    /// @implements ARCH-NO-DRIVING.no_model_call
    pending: Vec<Command>,
    pub running: bool,
    /// The live sandbox, if there is one: a copy of the project an agent the
    /// user started is working in.
    pub session: Option<tracelean_core::observe::sandbox::Session>,
    /// The agent's copy as it was last read, so a change can be reviewed
    /// against what the agent actually wrote.
    agent_tree: Option<Workspace>,
    /// Text for the frontend to put on the clipboard, taken once.
    pub clipboard: Option<String>,
    /// The explorer's open folders: the session's, not the workspace's.
    pub open_folders: BTreeSet<String>,
    /// Which parts of an agent's context the person has chosen to copy.
    pub context_parts: BTreeSet<tracelean_core::surface::context::Part>,
    /// What was last searched for, so the next match is one key away.
    pub last_find: Option<String>,
    /// The list `.` offered, while it is up: the pane and position it is
    /// about, and the offers in the order their keys name them.
    offering: Option<(String, usize, Vec<tracelean_core::surface::offer::Offer>)>,
    /// The trace index of the tree on disk, read once and kept until this
    /// editor writes to the disk: reading it parses every file, which is
    /// seconds on a real project, and a click should not wait for that.
    index_cache: std::cell::RefCell<Option<std::sync::Arc<tracelean_core::trace::index::Index>>>,
    /// How many findings the checker has on the disk, counted between keys
    /// (`tick`), and whether the disk changed since.
    problems: std::cell::Cell<Option<usize>>,
    problems_stale: std::cell::Cell<bool>,
    /// Whether closing was refused once already for unsaved work.
    close_warned: bool,
    /// What was last kept for next time (`recall`).
    remembered: Option<recall::Recalled>,
    /// The folders opened before, when a frontend keeps them (`track_recent`).
    recent: Option<Vec<String>>,
}

/// The keys that pick from `.`'s list, in order.
const OFFER_KEYS: [&str; 35] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
    "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
];

/// How many files a project can hold and still open with every folder open.
const SMALL_PROJECT: usize = 40;

/// What an action is about, at a position in a buffer.
///
/// Free of the editor because a frontend resolves actions against buffers that
/// are not the focused pane: a click on a row of the strip or the stations is
/// about *that* row, and resolving it against the pane would do something other
/// than the thing clicked.
///
/// @implements REQ-ACT.focus_is_carried
pub fn focus_in(buffer: &Buffer, offset: usize) -> Focus {
    let text: Vec<char> = buffer.text.chars().collect();
    let under = buffer
        .spans
        .iter()
        .find(|span| span.start <= offset && offset < span.stop)
        .map(|span| text[span.start..span.stop.min(text.len())].iter().collect::<String>());
    Focus { kind: buffer.kind.clone(), offset, under }
}

/// One pane, ready to be drawn.
///
/// `buffer` is whole rather than windowed: see `Editor::laid_out`.
#[derive(Debug, Clone)]
pub struct Placed {
    /// The pane's identity, which is what an action names.
    pub pane: String,
    /// Where it sits in the region that was laid out.
    pub at: Rect,
    /// What it shows.
    pub buffer: Buffer,
    /// Its first shown line.
    pub top: usize,
    /// Whether this is the pane a key goes to.
    pub focused: bool,
}

/// The mode in which a key is text rather than a command.
///
/// Named here because both the keymap and this file have to agree on it: the
/// keymap says which key enters it, and this says what happens once inside.
pub const INSERT: &str = "Insert";

/// The chords `Editor::chord` and the frontends answer, as the keys list
/// (F1) reads them.
const CHORDS: &[(&str, &str)] = &[
    ("Ctrl+S", "save"),
    ("Ctrl+Z / Ctrl+Y", "undo / redo, a word at a time"),
    ("Ctrl+W", "close the tab"),
    ("Ctrl+Tab", "next tab of the pane"),
    ("Ctrl+P", "open a file by a few letters; name:42 or :42 for a line"),
    ("Ctrl+Shift+P", "do anything, by a few words of its name"),
    ("Ctrl+Shift+O", "go to a name this file declares (the window only)"),
    ("Ctrl+F / F3", "find in the buffer / again"),
    ("Ctrl+Shift+F", "find in every file"),
    ("Ctrl+H (Ctrl+R in a terminal)", "replace every match in the file (one undo)"),
    ("F12, Ctrl+click", "go to the definition"),
    ("Alt+Left / Alt+Right", "back / forward to where a jump left from"),
    ("Shift+arrows, drag", "select"),
    ("Ctrl+A / Ctrl+C / Ctrl+X", "select all / copy / cut"),
    ("Ctrl+L", "select the line (again: the next one too)"),
    ("Ctrl+D", "select the word (again: its next occurrence)"),
    ("Home", "to the first character, then the line's start"),
    ("Ctrl+/", "comment the line or selection"),
    ("Tab / Shift+Tab", "indent / dedent the line or selection"),
    ("Alt+Up / Alt+Down", "move the line"),
    ("Shift+Alt+Down", "copy the line"),
    ("Ctrl+Shift+K", "delete the line"),
    ("Ctrl+Left / Right", "move by word"),
    ("Ctrl+Backspace", "delete a word"),
    ("Left / Right on the explorer", "close / open a folder; Left on the top closes every folder"),
    ("Ctrl+= / Ctrl+- / Ctrl+0", "zoom the window in / out / back (the window only)"),
    ("right-click, .", "what can be done here"),
    ("F1", "this list"),
];

impl Editor {
    /// The claims to mark beside a window of a file buffer (`surface::chips`),
    /// as the buffer line, the role's letter and the requirement each opens.
    /// None for a buffer that is not a file.
    ///
    /// Finding them parses the whole file, which on a long one took longer
    /// than the key that changed it; so they are counted when a file is first
    /// drawn, kept while it is typed into, and counted again between keys
    /// (`tick`).
    pub fn chips_shown(&self, buffer: &Buffer, top: usize, height: usize) -> Vec<(usize, char, String)> {
        let BufferKind::File { path } = &buffer.kind else { return Vec::new() };
        // A requirement's own document: its clauses, marked with what claims
        // them anywhere in the tree.
        let index = self.index();
        if let Some(requirement) = index.requirements.values().find(|r| r.file == *path) {
            let claims: Vec<(Option<String>, tracelean_core::trace::annotation::Role)> = index
                .links
                .iter()
                .filter(|l| l.req_id == requirement.id)
                .map(|l| (l.clause.clone(), l.role))
                .collect();
            return tracelean_core::surface::chips::clause_chips(&buffer.text, &requirement.id, &claims)
                .into_iter()
                .filter(|c| (top..top + height).contains(&c.line))
                .map(|c| (c.line, c.letter, c.requirement))
                .collect();
        }
        let mut held = self.chips_held.borrow_mut();
        let (_, chips) = held
            .entry(path.clone())
            .or_insert_with(|| (buffer.text.clone(), tracelean_core::surface::chips::chips(path, &buffer.text)));
        chips
            .iter()
            .filter(|c| (top..top + height).contains(&c.line))
            .map(|c| (c.line, c.letter, c.requirement.clone()))
            .collect()
    }

    /// Count again the chips of each file whose text moved on since; whether
    /// any did.
    fn recount_chips(&self) -> bool {
        let workspace = self.workspace();
        let mut held = self.chips_held.borrow_mut();
        let mut moved = false;
        for (path, (text, chips)) in held.iter_mut() {
            let Some(now) = workspace.files.get(path) else { continue };
            if now != text {
                *text = now.clone();
                *chips = tracelean_core::surface::chips::chips(path, now);
                moved = true;
            }
        }
        moved
    }

    /// Open a working tree, with what was open there last time.
    pub fn open(root: PathBuf, keymap: Keymap) -> Editor {
        Editor::open_as(root, keymap, true)
    }

    /// Open a working tree as if for the first time, and keep nothing for
    /// next time: what a suite driving a shared tree wants.
    pub fn open_fresh(root: PathBuf, keymap: Keymap) -> Editor {
        Editor::open_as(root, keymap, false)
    }

    fn open_as(root: PathBuf, keymap: Keymap, recalling: bool) -> Editor {
        let workspace = tracelean_core::observe::workspace::snapshot(&root);
        let empty = make::menu_buffer("empty".into(), vec![]);
        let mut editor = Editor {
            root,
            mode: keymap.root.clone(),
            keymap,
            screen: Screen {
                layout: Layout::Pane { id: "pane0".into(), buffer: empty.id.clone() },
                opened: vec![empty],
                focus: "pane0".into(),
                next_pane: 1,
            },
            // What a frontend that has not said its size yet is given. The
            // first draw replaces it; until then a focus move has nowhere to
            // go, which is the right answer for a screen of no size.
            region: Rect { left: 0, top: 0, width: 0, height: 0 },
            offset: 0,
            anchor: None,
            status: make::record_buffer("status".into(), vec![]),
            menu: None,
            top: 0,
            parked: BTreeMap::new(),
            places: BTreeMap::new(),
            back: Vec::new(),
            forward: Vec::new(),
            tree: Tree::new(workspace.clone()),
            held_state: std::cell::RefCell::new(None),
            chips_held: std::cell::RefCell::new(BTreeMap::new()),
            saved: workspace,
            pending: Vec::new(),
            running: true,
            session: None,
            agent_tree: None,
            clipboard: None,
            open_folders: BTreeSet::new(),
            context_parts: tracelean_core::surface::context::default_parts(),
            last_find: None,
            offering: None,
            index_cache: std::cell::RefCell::new(None),
            problems: std::cell::Cell::new(None),
            problems_stale: std::cell::Cell::new(true),
            close_warned: false,
            remembered: None,
            recent: None,
        };
        // A sandbox outlives the editor that made it: the agent may still be
        // working in it when the window is reopened.
        // A small project opens with every folder open, so all of it is seen
        // at once; a large one opens closed, so the explorer stays readable.
        let files: Vec<String> = editor.workspace().files.keys().cloned().collect();
        if files.len() <= SMALL_PROJECT {
            editor.open_folders = files.iter().flat_map(|f| explorer::holders(f)).collect();
        }
        editor.session = tracelean_core::observe::workcopy::list(&editor.root).into_iter().next();
        if let Some(live) = &editor.session {
            let host = tracelean_core::observe::workcopy::host(&editor.root);
            let _ = tracelean_core::observe::workcopy::write_launcher(live, &host);
        }
        // The first TraceLean's arrangement: what you choose from on the left,
        // what you read in the middle, what you consult on the right. Built by
        // the core, so the terminal and the window open on the same screen.
        let listing = editor.listing();
        let side = editor.produce(BufferKind::Menu { title: "requirements".into() });
        let welcome = editor.welcome();
        editor.screen = workbench(listing, welcome, side);
        if recalling {
            if let Some(kept) = recall::load(&editor.root) {
                editor.restore(kept);
            }
            editor.remembered = Some(editor.recall());
        }
        editor.say("ready", "F1 lists every key; . lists what can be done here, space opens the leader menu, i inserts");
        editor
    }

    /// The buffer the focused pane shows.
    ///
    /// Read out of `screen.opened` rather than kept beside it. A second copy is
    /// a second answer to what the buffer contains, and the two would drift the
    /// first time one of them was edited.
    pub fn buffer(&self) -> Buffer {
        let shown = panes(&self.screen.layout)
            .into_iter()
            .find(|(pane, _)| *pane == self.screen.focus)
            .map(|(_, buffer)| buffer);
        match shown {
            None => make::menu_buffer("empty".into(), vec![]),
            Some(id) => self
                .screen
                .opened
                .iter()
                .find(|held| held.id == id)
                .cloned()
                .unwrap_or_else(|| make::menu_buffer("empty".into(), vec![])),
        }
    }

    /// Replace the focused pane's buffer with this one, keeping its identity.
    ///
    /// Used where the editor changes what a buffer *contains* — re-reading a
    /// file after an edit — rather than what a pane points at.
    fn replace_buffer(&mut self, buffer: Buffer) {
        let current = self.buffer().id;
        match self.screen.opened.iter_mut().find(|held| held.id == current) {
            Some(held) => *held = buffer,
            None => self.screen.opened.push(buffer),
        }
    }

    /// The workspace as it is now: the base plus everything done to it.
    pub fn workspace(&self) -> Workspace {
        let here = self.tree.current();
        if let Some((at, state)) = self.held_state.borrow().as_ref() {
            if *at == here {
                return state.clone();
            }
        }
        let state = self.tree.state().unwrap_or_else(|_| self.tree.base.clone());
        *self.held_state.borrow_mut() = Some((here, state.clone()));
        state
    }

    /// Record a command from the state held, and hold the one after it.
    fn push(
        &mut self,
        command: Command,
    ) -> Result<tracelean_core::history::tree::NodeId, tracelean_core::history::command::Refusal> {
        let state = self.workspace();
        let (id, after) = self.tree.push_from(&state, command)?;
        *self.held_state.borrow_mut() = Some((Some(id), after));
        Ok(id)
    }

    /// One line of chrome, which is a record buffer so that it too is checkable.
    pub fn say(&mut self, kind: &str, text: &str) {
        self.status = make::record_buffer(
            "status".into(),
            vec![Event { kind: kind.to_string(), text: text.to_string() }],
        );
    }

    fn listing(&self) -> Buffer {
        self.listing_of(".")
    }

    /// The explorer's rows as they stand: the workspace's files, with the
    /// folders that are open showing what they hold.
    fn tree_rows(&self) -> Vec<explorer::Row> {
        let files: Vec<String> = self.workspace().files.keys().cloned().collect();
        explorer::rows(&files, &self.open_folders)
    }

    /// Whether a buffer is the explorer's tree rather than a flat listing.
    fn is_tree(&self, buffer: &Buffer) -> bool {
        matches!(&buffer.kind, BufferKind::Directory { path } if *path == self.root.display().to_string())
    }

    /// The row of the tree a position is on.
    fn tree_row_at(&self, buffer: &Buffer, offset: usize) -> Option<explorer::Row> {
        if !self.is_tree(buffer) {
            return None;
        }
        explorer::path_at(&self.tree_rows(), &buffer.text, offset).cloned()
    }

    /// Open a closed folder or close an open one, and redraw the tree where
    /// it is.
    fn toggle_folder(&mut self, path: &str) {
        if !self.open_folders.remove(path) {
            self.open_folders.insert(path.to_string());
        }
        self.refresh_views();
    }

    /// Left and Right on the explorer, as a file tree takes them: Right opens
    /// a closed folder and enters an open one; Left closes an open folder,
    /// else goes to the folder holding the row, and on a top-level row closes
    /// every folder. Whether the cursor was on the tree.
    fn tree_step(&mut self, opening: bool) -> bool {
        let buffer = self.buffer();
        let Some(row) = self.tree_row_at(&buffer, self.offset) else { return false };
        let target = match (opening, row.folder, row.open) {
            (true, true, false) | (false, true, true) => {
                self.toggle_folder(&row.path);
                row.path.clone()
            }
            (true, true, true) => {
                let rows = self.tree_rows();
                let at = rows.iter().position(|r| r.path == row.path);
                match at.and_then(|n| rows.get(n + 1)).filter(|next| next.depth > row.depth) {
                    Some(child) => child.path.clone(),
                    None => return true,
                }
            }
            (true, false, _) => return true,
            (false, _, _) => match row.path.rsplit_once('/') {
                Some((parent, _)) => parent.to_string(),
                None => {
                    self.open_folders.clear();
                    self.refresh_views();
                    self.say("folders closed", "every folder of the explorer is closed");
                    row.path.clone()
                }
            },
        };
        let buffer = self.buffer();
        let rows = self.tree_rows();
        let mut start = 0;
        for line in buffer.text.split('\n') {
            if explorer::path_at(&rows, &buffer.text, start).is_some_and(|r| r.path == target) {
                let lead = line.chars().take_while(|c| *c == ' ' || *c == '▸' || *c == '▾').count();
                self.offset = start + lead;
                return true;
            }
            start += line.chars().count() + 1;
        }
        true
    }

    /// Whether a path is a folder of the workspace: something holds files
    /// under it, and it is not itself a file.
    fn is_folder(&self, path: &str) -> bool {
        let workspace = self.workspace();
        let inside = format!("{}/", path.trim_end_matches('/'));
        !workspace.files.contains_key(path) && workspace.files.keys().any(|f| f.starts_with(&inside))
    }

    /// The files under a path, as a buffer.
    ///
    /// The path is honoured rather than ignored. It was ignored until the
    /// requirements station opened and showed the whole tree: every directory
    /// asked for produced the same listing, which is a producer that answers a
    /// question it was not asked, and nothing could have noticed while only one
    /// directory was ever asked for.
    ///
    /// @implements REQ-SHOW.listing_from_entries
    fn listing_of(&self, path: &str) -> Buffer {
        let under = path.trim_end_matches('/');
        if under == "." {
            // A file made and then written is still a file made: the first
            // command on a path says what happened to it.
            let mut changed: BTreeMap<String, String> = BTreeMap::new();
            for command in &self.pending {
                let kind = match command {
                    Command::CreateFile { .. } => "created",
                    Command::DeleteFile { .. } => "deleted",
                    _ => "modified",
                };
                changed.entry(touched(command)).or_insert_with(|| kind.to_string());
            }
            // Edits here not yet written out, unless an agent's mark says more.
            for path in self.unsaved() {
                changed.entry(path).or_insert_with(|| "unsaved".to_string());
            }
            // The file the document pane shows.
            let current = self
                .laid_out(self.region)
                .into_iter()
                .find(|p| p.pane == tracelean_core::surface::screen::DOCUMENT)
                .and_then(|p| match p.buffer.kind {
                    BufferKind::File { path } => Some(path),
                    _ => None,
                });
            // What each file claims, by kind: the letters after its name.
            let mut claimed: BTreeMap<String, BTreeSet<tracelean_core::trace::annotation::Role>> = BTreeMap::new();
            for link in &self.index().links {
                claimed.entry(link.anchor.file.clone()).or_default().insert(link.role);
            }
            return explorer::tree_buffer(
                self.root.display().to_string(),
                &self.tree_rows(),
                &changed,
                &claimed,
                current.as_deref(),
            );
        }
        let prefix = if under == "." { String::new() } else { format!("{under}/") };
        let mut entries: Vec<(usize, String)> = self
            .workspace()
            .files
            .keys()
            .filter(|held| prefix.is_empty() || held.starts_with(&prefix))
            .map(|held| (0, held.clone()))
            .collect();
        entries.sort();
        let title =
            if under == "." { self.root.display().to_string() } else { under.to_string() };
        tracelean_core::surface::view::directory_buffer(title, entries)
    }

    /// The text of the span under the cursor, which is how an action learns
    /// what it is about.
    ///
    /// A row of the opened strip is about the buffer it lists, which its text
    /// — a number and a title — does not spell; so there the target is the
    /// opened buffer at that row.
    pub fn focus(&self) -> Focus {
        let buffer = self.buffer();
        let mut focus = focus_in(&buffer, self.offset);
        if buffer.id == "menu:opened" {
            focus.under = self.opened_at_row(&buffer, self.offset).map(|held| held.id);
        }
        // A button on a row of waiting changes is about that row's file.
        if buffer.id == "record:sandbox" {
            let here = actions_at(buffer.clone(), self.offset);
            if here.iter().any(|a| a == "observe.accept_file" || a == "observe.reject_file") {
                focus.under = changed_on_row(&buffer, self.offset);
            }
        }
        // A row of the tree shows a name; what it is about is its whole path.
        // Its first line names the project and is about nothing to open.
        if self.is_tree(&buffer) {
            focus.under = self.tree_row_at(&buffer, self.offset).map(|row| row.path);
        }
        // An opened requirement's Approve button is about its document, the
        // view's third line.
        let requirement = matches!(&buffer.kind, BufferKind::Record { title } if title.starts_with("requirement "));
        if requirement && actions_at(buffer.clone(), self.offset).iter().any(|a| a == "trace.approve") {
            focus.under = tracelean_core::surface::view::plain_text(buffer).get(2).cloned();
        }
        focus
    }

    /// The opened buffer a row of a strip lists.
    fn opened_at_row(&self, strip: &Buffer, offset: usize) -> Option<Buffer> {
        let row = strip.text.chars().take(offset).filter(|c| *c == '\n').count();
        self.screen.opened.get(row).cloned()
    }

    /// An action from a row of one of the two bars: the stations, or the strip
    /// of what is opened.
    ///
    /// A strip row shows its buffer where that kind of buffer belongs, as a
    /// tab does; anything else is dispatched with the bar's own row as the
    /// focus, so a click on a station opens that station.
    pub fn act_in_bar(&mut self, station: bool, action: &str, offset: usize) {
        if !station && action == "screen.show" {
            let strip = self.strip();
            match self.opened_at_row(&strip, offset) {
                Some(held) => {
                    let title = tracelean_core::surface::screen::title_of(&held);
                    self.show(held);
                    self.say("shown", &title);
                }
                None => self.say("nothing happened", "that row lists nothing"),
            }
            return;
        }
        let buffer = if station { self.stations() } else { self.strip() };
        let focus = focus_in(&buffer, offset);
        let intent = dispatch(action.to_string(), focus, self.workspace());
        self.perform(intent);
    }

    /// Ctrl+Tab: show the next tab that opens where the focused pane is (the
    /// previous one backwards), so cycling stays in the pane being read.
    fn cycle_tab(&mut self, forward: bool) {
        let here = self.buffer().id;
        let mates: Vec<Buffer> = self
            .screen
            .opened
            .iter()
            .filter(|b| destination(b.kind.clone(), self.screen.clone()) == self.screen.focus)
            .cloned()
            .collect();
        let Some(at) = mates.iter().position(|b| b.id == here) else { return };
        let next = if forward { (at + 1) % mates.len() } else { (at + mates.len() - 1) % mates.len() };
        if next != at {
            let title = tracelean_core::surface::screen::title_of(&mates[next]);
            self.show(mates[next].clone());
            self.say("shown", &title);
        }
    }

    /// What the cursor can do where it is, which is what a rendered affordance
    /// would carry at the same place.
    pub fn offered(&self) -> Vec<String> {
        actions_at(self.buffer(), self.offset)
    }

    /// Open a buffer and show it in the focused pane, cursor at the start.
    ///
    /// A buffer already opened is replaced by the one just produced rather than
    /// added beside it: they have the same identity, so two of them would be two
    /// answers to what that identity holds.
    ///
    /// The focused pane is first moved to where a buffer of this kind belongs —
    /// a file to the document, a listing to the explorer, a station to the side
    /// panel — through the same arrangement a key or a pointer would make.
    ///
    /// @implements REQ-SCREEN.buffer_goes_home
    fn show(&mut self, buffer: Buffer) {
        let home = destination(buffer.kind.clone(), self.screen.clone());
        // The document leaving a file for anything else is a jump to come
        // back from.
        if home == tracelean_core::surface::screen::DOCUMENT
            && self.document_place().is_some_and(|(path, _)| buffer.kind != BufferKind::File { path })
        {
            self.mark_jump();
        }
        if home != self.screen.focus {
            self.rearrange(Arrangement::FocusPane { pane: home.clone() });
        }
        self.places.insert(self.buffer().id, (self.offset, self.top));
        self.anchor = None;
        let id = buffer.id.clone();
        let length = buffer.text.chars().count();
        match self.screen.opened.iter_mut().find(|held| held.id == id) {
            Some(held) => *held = buffer,
            None => self.screen.opened.push(buffer),
        }
        let (offset, top) = self.places.get(&id).copied().unwrap_or((0, 0));
        self.screen = tracelean_core::surface::screen::show_buffer(id, self.screen.clone());
        self.offset = offset.min(length);
        self.top = top;
        // The tree marks the file the document shows, and the trace, if it is
        // open, follows it.
        if home == tracelean_core::surface::screen::DOCUMENT {
            let tree = self.listing_of(".");
            if let Some(held) = self.screen.opened.iter_mut().find(|held| held.id == tree.id) {
                *held = tree;
            }
            if matches!(self.buffer().kind, BufferKind::File { .. }) && self.screen.opened.iter().any(|b| b.id == "record:trace") {
                let trace = self.file_trace();
                if let Some(held) = self.screen.opened.iter_mut().find(|held| held.id == trace.id) {
                    *held = trace;
                }
            }
        }
    }

    /// The opened set, as a buffer.
    ///
    /// A rendering of what the session holds rather than a list kept beside it,
    /// which is why it is the core's function and not a loop here.
    ///
    /// @implements REQ-SCREEN.strip_is_the_opened_set
    pub fn strip(&self) -> Buffer {
        tracelean_core::surface::screen::strip_with_unsaved(self.screen.clone(), &self.unsaved())
    }

    /// What to have back next time: the files in the tabs, the open folders,
    /// and the file in the document pane.
    pub fn recall(&self) -> recall::Recalled {
        let file = |b: &Buffer| match &b.kind {
            BufferKind::File { path } => Some(path.clone()),
            _ => None,
        };
        let shown = panes(&self.screen.layout)
            .into_iter()
            .find(|(pane, _)| pane == tracelean_core::surface::screen::DOCUMENT)
            .and_then(|(_, id)| self.screen.opened.iter().find(|b| b.id == id).and_then(file));
        recall::Recalled {
            files: self.screen.opened.iter().filter_map(file).collect(),
            folders: self.open_folders.iter().cloned().collect(),
            shown,
        }
    }

    /// Keep what `recall` says, when it changed since it was last kept — and
    /// never for an editor opened fresh.
    pub fn remember(&mut self) {
        let Some(kept) = &self.remembered else { return };
        let now = self.recall();
        if *kept != now {
            recall::save(&self.root, &now);
            self.remembered = Some(now);
        }
    }

    /// Open again what was open last time, leaving out what no longer exists.
    fn restore(&mut self, kept: recall::Recalled) {
        let files = self.workspace().files;
        self.open_folders = kept.folders.into_iter().filter(|f| self.is_folder(f)).collect();
        let last = kept.shown.into_iter().filter(|p| files.contains_key(p));
        for path in kept.files.into_iter().filter(|p| files.contains_key(p)).chain(last) {
            self.perform(Intent::Display { what: BufferKind::File { path } });
        }
    }

    /// The files whose text here differs from the disk: changed, made or
    /// removed and not yet saved.
    pub fn unsaved(&self) -> Vec<String> {
        let current = self.workspace();
        let mut paths: Vec<String> = current
            .files
            .iter()
            .filter(|(path, text)| self.saved.files.get(*path) != Some(*text))
            .map(|(path, _)| path.clone())
            .collect();
        paths.extend(self.saved.files.keys().filter(|p| !current.files.contains_key(*p)).cloned());
        paths.sort();
        paths
    }

    /// Whether the window may close now. With unsaved work it may not, the
    /// first time: the status line says what would be lost, and asking again
    /// closes anyway — a person who has been told decides.
    pub fn may_close(&mut self) -> bool {
        let unsaved = self.unsaved();
        if unsaved.is_empty() || self.close_warned {
            self.remember();
            return true;
        }
        self.close_warned = true;
        self.say(
            "not closed",
            &format!("unsaved: {} — Ctrl+S saves; close again to discard", unsaved.join(", ")),
        );
        false
    }

    /// The stations, as a buffer.
    ///
    /// The same for every state, so that the station which opens a project is
    /// reachable when no project is open.
    ///
    /// @implements REQ-SCREEN.stations_are_constant
    pub fn stations(&self) -> Buffer {
        tracelean_core::surface::screen::stations(self.screen.clone())
    }

    /// Every pane, where it goes, what it shows, and where it is scrolled to.
    ///
    /// What a frontend draws. The buffer is whole: a frontend reserves some of
    /// a pane for its own chrome — a terminal spends a column on a divider, a
    /// window spends none — and only it knows how much, so it windows the
    /// buffer to the room it has left by calling `produce::window`. The
    /// windowing is still the core's function and the result is still a buffer,
    /// which is what `window_is_a_buffer` asks; what the frontend supplies is
    /// the one number it alone knows.
    ///
    /// Each pane keeps its own first line, so two panes on one buffer are not
    /// locked together.
    ///
    /// @implements REQ-SCREEN.layout_tiles_the_region
    pub fn laid_out(&self, rect: Rect) -> Vec<Placed> {
        let showing: BTreeMap<String, String> = panes(&self.screen.layout).into_iter().collect();
        place(rect, self.screen.layout.clone())
            .into_iter()
            .map(|(pane, at)| {
                let buffer = showing
                    .get(&pane)
                    .and_then(|id| self.screen.opened.iter().find(|held| held.id == *id))
                    .cloned()
                    .unwrap_or_else(|| make::menu_buffer("empty".into(), vec![]));
                let top = if pane == self.screen.focus {
                    self.top
                } else {
                    self.parked.get(&pane).map(|(_, top)| *top).unwrap_or(0)
                };
                let focused = pane == self.screen.focus;
                Placed { pane, at, buffer, top, focused }
            })
            .collect()
    }

    /// A pane's edge dragged by `cells` columns (or rows): the pane is focused
    /// and the divider beside it moved that many cells.
    ///
    /// A split's weights are proportions, often as coarse as 1 : 2 : 1, where
    /// one unit is a fifth of the window — so a drag of a few columns was cut
    /// to nothing by the floor of one. The weights are first restated as the
    /// cells each part already occupies, which draws exactly the same picture,
    /// and then a cell dragged is a unit moved.
    pub fn grab(&mut self, pane: &str, cells: i64) {
        self.perform(Intent::Arrange { how: Arrangement::FocusPane { pane: pane.to_string() } });
        if cells == 0 {
            return;
        }
        // Before the frontend has said how big it is there are no cells to
        // count, and the proportions are left as they are.
        if self.region.width > 0 && self.region.height > 0 {
            self.screen.layout = in_cells(self.region, self.screen.layout.clone());
        }
        self.perform(Intent::Arrange { how: Arrangement::Resize { amount: cells } });
    }

    /// What a frontend draws: the part of the buffer that fits.
    ///
    /// A window is a buffer, so a frontend showing forty lines of a long file
    /// is still drawing everything it was given — which is what keeps the
    /// conformance check meaning what it says.
    ///
    /// @implements REQ-SHOW.window_is_a_buffer
    pub fn visible(&self, height: usize) -> Buffer {
        make::window(self.buffer(), self.top, height)
    }

    /// Where the cursor is in the window, as a line and a column.
    ///
    /// `None` when the cursor is not in the window at all, which a frontend
    /// draws by simply not drawing a cursor.
    pub fn cursor_in(&self, height: usize) -> Option<(usize, usize)> {
        let (line, column) = self.line_and_column();
        if line < self.top || line >= self.top + height {
            return None;
        }
        Some((line - self.top, column))
    }

    /// The cursor's line and column in the whole buffer.
    pub fn line_and_column(&self) -> (usize, usize) {
        let mut line = 0usize;
        let mut column = 0usize;
        for (at, character) in self.buffer().text.chars().enumerate() {
            if at >= self.offset {
                break;
            }
            if character == '\n' {
                line += 1;
                column = 0;
            } else {
                column += 1;
            }
        }
        (line, column)
    }

    /// Scroll so that the cursor is on screen, and no further.
    pub fn follow_cursor(&mut self, height: usize) {
        if height == 0 {
            return;
        }
        let (line, _) = self.line_and_column();
        if line < self.top {
            self.top = line;
        } else if line >= self.top + height {
            self.top = line + 1 - height;
        }
    }

    /// Move the cursor one character.
    pub fn step(&mut self, forwards: bool) {
        let length = self.buffer().text.chars().count();
        self.offset = if forwards {
            (self.offset + 1).min(length)
        } else {
            self.offset.saturating_sub(1)
        };
    }

    /// Move the cursor one line, keeping the column where it can.
    pub fn step_line(&mut self, down: bool) {
        let lines: Vec<String> =
            tracelean_core::surface::view::plain_text(self.buffer());
        let (line, column) = self.line_and_column();
        let target = if down {
            (line + 1).min(lines.len().saturating_sub(1))
        } else {
            line.saturating_sub(1)
        };
        let width = lines.get(target).map(|l| l.chars().count()).unwrap_or(0);
        let mut at = 0usize;
        for before in lines.iter().take(target) {
            at += before.chars().count() + 1;
        }
        self.offset = at + column.min(width);
    }

    /// The file the cursor is editing, if it is editing one.
    ///
    /// Only a file buffer is editable: a listing, a menu and a report are
    /// produced from something else, and typing into one would be typing into a
    /// picture of the state rather than into the state.
    fn editing(&self) -> Option<String> {
        match self.buffer().kind {
            BufferKind::File { path } => Some(path),
            _ => None,
        }
    }

    /// Type one character at the cursor.
    ///
    /// Through a command, like every other change, so that it has an inverse
    /// and appears in the history.
    ///
    /// @implements REQ-ACT.edits_are_commands
    pub fn insert(&mut self, text: &str) {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return;
        };
        let at = self.offset;
        self.change(Command::Insert { file, offset: at, text: text.to_string() });
        self.offset = at + text.chars().count();
    }

    /// Delete the character before the cursor.
    ///
    /// The command carries what it removed, which is what gives it an inverse
    /// without going back to the workspace for it.
    ///
    /// @implements REQ-ACT.edits_are_commands
    pub fn delete_back(&mut self) {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return;
        };
        if self.offset == 0 {
            return;
        }
        // In a line's indentation it takes back one level, as Tab gave it.
        let (_, column) = self.line_and_column();
        let text: Vec<char> = self.buffer().text.chars().collect();
        let blank = column > 0 && text[self.offset - column..self.offset].iter().all(|c| *c == ' ');
        let step = self.indent_step();
        let width = if blank { (column - 1) % step + 1 } else { 1 };
        let at = self.offset - width;
        let deleted: String = text[at..self.offset].iter().collect();
        self.change(Command::Delete { file, offset: at, deleted });
        self.offset = at;
    }

    /// Apply a change to the file being edited and redraw it, keeping the
    /// cursor where the caller put it.
    fn change(&mut self, command: Command) {
        let Some(path) = self.editing() else { return };
        let was_saved = self.saved.files.get(&path).map(String::as_str) == Some(self.buffer().text.as_str());
        match self.push(command) {
            Ok(_) => {
                let text = self.workspace().files.get(&path).cloned().unwrap_or_default();
                let marks = marks_in(&path, &text);
                let saved = self.saved.files.get(&path) == Some(&text);
                // In place: the buffer's identity is the file's path and that
                // has not changed, so this is new content for the buffer the
                // pane already points at, not a different buffer.
                self.replace_buffer(make::file_buffer(path, text, marks));
                // The explorer marks a file with unsaved edits.
                if saved != was_saved {
                    self.refresh_views();
                }
            }
            Err(refusal) => self.say("refused", &format!("{refusal:?}")),
        }
    }

    /// Re-produce every opened listing and file from the workspace as it now
    /// is, without moving the focus or rearranging anything.
    ///
    /// After an undo or a rename the explorer and the open file show the new
    /// state where they already are; re-showing the listing used to pull the
    /// focus into the explorer and leave the document showing the old text.
    fn refresh_views(&mut self) {
        let workspace = self.workspace();
        let fresh: Vec<Buffer> = self
            .screen
            .opened
            .iter()
            .map(|held| match &held.kind {
                // The root listing is titled with the tree's own path rather
                // than `.`, so that is what its kind carries.
                BufferKind::Directory { path } if *path == self.root.display().to_string() => {
                    self.listing_of(".")
                }
                BufferKind::Directory { path } => self.listing_of(path),
                // The history marks where the workspace is, which every
                // change and every travel moves.
                BufferKind::Record { title } if title == "history" => {
                    tracelean_core::surface::history_view::history_view(&self.points())
                }
                BufferKind::File { path } => match workspace.files.get(path) {
                    Some(text) => make::file_buffer(path.clone(), text.clone(), marks_in(path, text)),
                    None => held.clone(),
                },
                _ => held.clone(),
            })
            .collect();
        // A listing's identity is its title, which is the path it lists, so a
        // fresh one replaces the old one rather than joining it.
        for (held, new) in self.screen.opened.iter_mut().zip(fresh) {
            if held.id == new.id {
                *held = new;
            }
        }
        let length = self.buffer().text.chars().count();
        self.offset = self.offset.min(length);
    }

    /// The buffer a pane shows, whole.
    fn buffer_in(&self, pane: &str) -> Option<Buffer> {
        let id = panes(&self.screen.layout).into_iter().find(|(p, _)| p == pane)?.1;
        self.screen.opened.iter().find(|held| held.id == id).cloned()
    }

    /// The first line a pane shows.
    fn top_of(&self, pane: &str) -> usize {
        if pane == self.screen.focus {
            self.top
        } else {
            self.parked.get(pane).map(|(_, top)| *top).unwrap_or(0)
        }
    }

    /// A position a frontend reported in the window it drew, as a position in
    /// the whole buffer. A frontend is handed the visible lines only, so its
    /// offsets start at the first visible line.
    fn absolute(&self, pane: &str, at: usize) -> usize {
        let Some(buffer) = self.buffer_in(pane) else { return at };
        let top = self.top_of(pane);
        let lines = tracelean_core::surface::view::plain_text(buffer.clone());
        let shift: usize = lines.iter().take(top).map(|line| line.chars().count() + 1).sum();
        (shift + at).min(buffer.text.chars().count())
    }

    /// Move the focus to a pane, through the one arrangement path.
    pub fn focus_pane(&mut self, pane: &str) {
        if pane != self.screen.focus {
            self.rearrange(Arrangement::FocusPane { pane: pane.to_string() });
        }
    }

    /// A pointer pressed at a position: focus that pane and put the cursor
    /// there. Pressing into a file starts typing, as it does in any editor;
    /// pressing into anything else leaves keys as commands.
    pub fn place(&mut self, pane: &str, at: usize) {
        let offset = self.absolute(pane, at);
        self.focus_pane(pane);
        self.offset = offset;
        self.anchor = None;
        self.menu = None;
        if self.editing().is_some() {
            self.mode = INSERT.to_string();
            self.say("insert", "type to edit · Escape for commands · right-click for everything else");
        } else {
            self.mode = self.keymap.root.clone();
        }
    }

    /// Everything that can be done at a position of a pane, for a context menu.
    ///
    /// @implements REQ-ACT.everything_is_offered
    pub fn offers_at(&self, pane: &str, at: usize) -> Vec<tracelean_core::surface::offer::Offer> {
        let Some(buffer) = self.buffer_in(pane) else { return Vec::new() };
        let mut offset = self.absolute(pane, at);
        // On the tree a row is about its name wherever on the row the cursor
        // is — in its indentation, say, where a keyboard leaves it.
        if self.is_tree(&buffer) {
            let chars: Vec<char> = buffer.text.chars().collect();
            let start = chars[..offset.min(chars.len())].iter().rposition(|c| *c == '\n').map_or(0, |n| n + 1);
            let end = chars[start..].iter().position(|c| *c == '\n').map_or(chars.len(), |n| start + n);
            if let Some(name) = buffer.spans.iter().find(|s| s.start >= start && s.start < end && !s.actions.is_empty()) {
                if offset < name.start || offset >= name.stop {
                    offset = name.start;
                }
            }
        }
        let mut offered = tracelean_core::surface::offer::offers(&buffer, offset, &self.keymap);
        // The core offers what a row's text names; on the tree that is a name,
        // and the target is the whole path the row stands for.
        if let Some(row) = self.tree_row_at(&buffer, offset) {
            if row.folder {
                offered.retain(|o| !matches!(o.action.as_str(), "file.rename" | "file.delete"));
            }
            for o in offered.iter_mut().filter(|o| o.target.as_deref() == Some(row.name.as_str())) {
                o.target = Some(row.path.clone());
                o.label = o.label.replace(row.name.as_str(), &row.path);
                o.asks = o.asks.as_ref().map(|q| q.replace(row.name.as_str(), &row.path));
                if row.folder && o.action == "file.open" {
                    o.label = if row.open { format!("Close {}", row.name) } else { format!("Open {}", row.name) };
                }
            }
        }
        // Anywhere inside an item that claims a clause, the clause it claims:
        // a function's body is about its requirement as much as the
        // annotation over it is.
        if let BufferKind::File { path } = &buffer.kind {
            let line = buffer.text.chars().take(offset).filter(|c| *c == '\n').count() as u32 + 1;
            let mut claimed: Vec<String> = self
                .index()
                .links
                .iter()
                .filter(|l| l.anchor.file == *path && l.anchor.precise)
                .filter(|l| l.line <= line && line <= l.anchor.end_line)
                .map(|l| match &l.clause {
                    Some(clause) => format!("{}.{clause}", l.req_id),
                    None => l.req_id.clone(),
                })
                .collect();
            claimed.dedup();
            let at = offered.iter().position(|o| o.group != "here").unwrap_or(offered.len());
            let mut extra = Vec::new();
            for clause in claimed {
                if offered.iter().any(|o| o.group == "here" && o.target.as_deref() == Some(clause.as_str())) {
                    continue;
                }
                for (action, label) in [
                    ("trace.requirement", format!("Open {clause}")),
                    ("trace.context", format!("Context for an agent to change {clause}")),
                ] {
                    extra.push(tracelean_core::surface::offer::Offer {
                        group: "here".into(),
                        label,
                        action: action.into(),
                        target: Some(clause.clone()),
                        asks: None,
                        // Chosen, not typed: a key acts on the span under
                        // the cursor, and here there is none naming it.
                        keys: None,
                    });
                }
            }
            offered.splice(at..at, extra);
        }
        offered
    }

    /// Everything that can be done where the cursor is, in the focused pane,
    /// with that pane and the cursor's offset in its window — what the
    /// which-key bar shows while no menu is open, and what an entry of it is
    /// then chosen at.
    pub fn offers_here(&self) -> (String, usize, Vec<tracelean_core::surface::offer::Offer>) {
        let pane = self.screen.focus.clone();
        let at = self.offset.saturating_sub(self.absolute(&pane, 0));
        let offered = self.offers_at(&pane, at);
        (pane, at, offered)
    }

    /// Perform an action chosen at a position of a pane — a click on an
    /// affordance, or an entry of the context menu.
    ///
    /// The focus is built from what was pointed at, as `focus_in` builds it for
    /// a key, and then the offer's target and the person's answer fill in what
    /// a pointer knows and a cursor does not: which row of a listing was meant,
    /// and the name typed for it. The dispatcher is the one every key goes
    /// through (`REQ-ACT.one_path`).
    pub fn choose(
        &mut self,
        pane: &str,
        at: usize,
        action: &str,
        target: Option<String>,
        answer: Option<String>,
    ) {
        let offset = self.absolute(pane, at);
        self.focus_pane(pane);
        self.offset = offset;
        let mut focus = self.focus();
        match action {
            "file.rename" => {
                if let Some(path) = target {
                    focus.kind = BufferKind::File { path };
                }
                focus.under = answer;
            }
            "file.delete" | "file.copy_path" => {
                if let Some(path) = target {
                    focus.kind = BufferKind::File { path };
                }
            }
            "file.new" | "trace.new_requirement" => focus.under = answer,
            _ => {
                // What was typed, else what was pointed at.
                if let Some(chosen) = answer.or(target) {
                    focus.under = Some(chosen);
                }
            }
        }
        let intent = dispatch(action.to_string(), focus, self.workspace());
        let approving = action == "trace.approve" && matches!(intent, Intent::Edit { .. });
        self.perform(intent);
        if approving {
            self.say("approved", "the requirement is approved in the editor; Ctrl+S writes it");
        }
        self.mode = self.keymap.root.clone();
        self.menu = None;
    }

    /// The keys every editor has and a modal keymap does not spell: save, undo,
    /// redo, and moving by line and by page.
    ///
    /// Each is the same action or the same cursor move a key sequence makes;
    /// this only gives it the name a person's hands already know.
    pub fn chord(&mut self, chord: &str) {
        // Shift with a movement selects from where the cursor was.
        let moves = ["Left", "Right", "Up", "Down", "Home", "End", "PageUp", "PageDown", "C-Left", "C-Right"];
        if let Some(movement) = chord.strip_prefix("S-").filter(|m| moves.contains(m)) {
            let anchor = self.anchor.unwrap_or(self.offset);
            match movement {
                "Left" => self.step(false),
                "Right" => self.step(true),
                "Up" => self.step_line(false),
                "Down" => self.step_line(true),
                other => self.chord(other),
            }
            self.anchor = Some(anchor);
            return;
        }
        if let Some(movement) = chord.strip_prefix("C-S-").filter(|m| ["Left", "Right"].contains(m)) {
            return self.chord(&format!("S-C-{movement}"));
        }
        match chord {
            "C-a" => {
                self.anchor = Some(0);
                self.offset = self.buffer().text.chars().count();
                return;
            }
            "C-c" | "C-x" => {
                match self.selected() {
                    Some(text) => {
                        self.say("copied", &format!("{} character(s)", text.chars().count()));
                        self.clipboard = Some(text);
                        if chord == "C-x" {
                            self.take_selection();
                        }
                    }
                    None => self.say("nothing selected", "select with Shift and the arrows, or drag"),
                }
                return;
            }
            "Delete" if self.take_selection() => return,
            // From the match selected, to the next.
            "F3" | "C-g" => return self.find_again(true),
            "S-F3" | "C-S-g" => return self.find_again(false),
            // The word at the cursor; again, the next place that holds what
            // is selected (and F3 on from there).
            "C-d" => {
                match self.selected().filter(|s| !s.is_empty()) {
                    Some(text) => self.find(&text, true),
                    None => {
                        let chars: Vec<char> = self.buffer().text.chars().collect();
                        let part = |c: &char| c.is_alphanumeric() || *c == '_';
                        let at = self.offset.min(chars.len());
                        let start = at - chars[..at].iter().rev().take_while(|c| part(c)).count();
                        let end = at + chars[at..].iter().take_while(|c| part(c)).count();
                        if start < end {
                            self.anchor = Some(start);
                            self.offset = end;
                            self.last_find = Some(chars[start..end].iter().collect());
                        }
                    }
                }
                return;
            }
            // The line, whole; again, the next one with it.
            "C-l" => {
                let (line, column) = self.line_and_column();
                let lines = tracelean_core::surface::view::plain_text(self.buffer());
                let start = self.offset - column;
                let length = lines.get(line).map_or(0, |l| l.chars().count());
                let total = self.buffer().text.chars().count();
                self.anchor = Some(self.anchor.unwrap_or(start));
                self.offset = (start + length + 1).min(total);
                return;
            }
            "S-Tab" | "C-/" => {
                if let Some((from, to)) = self.selection() {
                    return self.over_range(from, to, chord);
                }
            }
            _ => {}
        }
        self.anchor = None;
        let action = match chord {
            "C-s" => Some("file.save"),
            "C-z" => Some("history.undo"),
            "C-y" | "C-S-z" => Some("history.redo"),
            "C-w" => Some("screen.close"),
            "F12" => Some("file.definition"),
            _ => None,
        };
        if let Some(action) = action {
            let intent = dispatch(action.to_string(), self.focus(), self.workspace());
            self.perform(intent);
            return;
        }
        let page = (self.region.height as usize).saturating_sub(1).max(1);
        match chord {
            // To the line's first character that is not blank, and from
            // there to its very start.
            "Home" => {
                let (line, column) = self.line_and_column();
                let lines = tracelean_core::surface::view::plain_text(self.buffer());
                let text = lines.get(line).map(String::as_str).unwrap_or("");
                let blank = text.chars().take_while(|c| c.is_whitespace()).count();
                let to = if column == blank { 0 } else { blank };
                self.offset = self.offset - column + to;
            }
            "End" => {
                let (line, column) = self.line_and_column();
                let lines = tracelean_core::surface::view::plain_text(self.buffer());
                let width = lines.get(line).map(|l| l.chars().count()).unwrap_or(0);
                self.offset += width - column.min(width);
            }
            "PageUp" => (0..page).for_each(|_| self.step_line(false)),
            "PageDown" => (0..page).for_each(|_| self.step_line(true)),
            "Delete" => self.delete_forward(),
            "C-Left" => self.offset = self.word_edge(false),
            "C-Right" => self.offset = self.word_edge(true),
            "C-Backspace" => self.delete_to(self.word_edge(false)),
            "C-Delete" => self.delete_to(self.word_edge(true)),
            "C-/" => self.toggle_comment(),
            "A-Up" => self.move_line(false),
            "A-Down" => self.move_line(true),
            "A-S-Down" | "A-S-Up" => self.duplicate_line(),
            "C-S-k" => self.delete_line(),
            "S-Tab" => self.dedent_line(),
            "F1" => self.perform(Intent::Display { what: BufferKind::Record { title: "keys".into() } }),
            "A-Left" => self.go_back(true),
            "A-Right" => self.go_back(false),
            "C-Tab" => self.cycle_tab(true),
            "C-S-Tab" => self.cycle_tab(false),
            "F3" | "C-g" => self.find_again(true),
            "S-F3" | "C-S-g" => self.find_again(false),
            other => self.say("nothing happened", &format!("`{other}` is not bound here")),
        }
    }

    /// Replace every `wanted` in the focused file with `with`, as one change.
    pub fn replace_all(&mut self, wanted: &str, with: &str) {
        let Some(file) = self.editing() else {
            return self.say("nothing replaced", "replacing works in a file");
        };
        let text: Vec<char> = self.buffer().text.chars().collect();
        let needle: Vec<char> = wanted.chars().collect();
        let mut hits = Vec::new();
        let mut at = 0;
        while !needle.is_empty() && at + needle.len() <= text.len() {
            if text[at..].starts_with(&needle) {
                hits.push(at);
                at += needle.len();
            } else {
                at += 1;
            }
        }
        if hits.is_empty() {
            return self.say("not found", &format!("`{wanted}` is not in this buffer"));
        }
        // From the last match back, so each offset still means what it did;
        // one command, so one Ctrl+Z takes every replacement back.
        let mut commands = Vec::new();
        for &at in hits.iter().rev() {
            commands.push(Command::Delete { file: file.clone(), offset: at, deleted: wanted.to_string() });
            if !with.is_empty() {
                commands.push(Command::Insert { file: file.clone(), offset: at, text: with.to_string() });
            }
        }
        self.anchor = None;
        self.change(Command::Batch { commands });
        let length = self.buffer().text.chars().count();
        self.offset = self.offset.min(length);
        self.say("replaced", &format!("{} × `{wanted}` with `{with}` (Ctrl+Z takes them back)", hits.len()));
    }

    /// Move the cursor to the next place the focused buffer holds `wanted`,
    /// after the cursor and round to the start; backwards when not `forward`.
    pub fn find(&mut self, wanted: &str, forward: bool) {
        if wanted.is_empty() {
            return;
        }
        self.last_find = Some(wanted.to_string());
        let text: Vec<char> = self.buffer().text.chars().collect();
        let needle: Vec<char> = wanted.chars().collect();
        let hits: Vec<usize> = (0..text.len().saturating_sub(needle.len() - 1))
            .filter(|at| text[*at..].starts_with(&needle))
            .collect();
        // From the start of what is selected — the last match, after a find.
        let from = self.selection().map_or(self.offset, |(start, _)| start);
        let found = if forward {
            hits.iter().find(|at| **at > from).or(hits.first())
        } else {
            hits.iter().rev().find(|at| **at < from).or(hits.last())
        };
        match found {
            Some(at) => {
                let at = *at;
                let place = hits.iter().position(|h| *h == at).unwrap_or(0) + 1;
                // The match is selected, so typing replaces it.
                self.anchor = Some(at);
                self.offset = at + needle.len();
                let height = (self.region.height as usize).max(1);
                self.follow_cursor(height);
                self.say("found", &format!("{wanted}: {place} of {}; F3 for the next", hits.len()));
            }
            None => self.say("not found", &format!("{wanted} is not in this buffer")),
        }
    }

    fn find_again(&mut self, forward: bool) {
        match self.last_find.clone() {
            Some(wanted) => self.find(&wanted, forward),
            None => self.say("nothing to find", "Ctrl+F asks what to look for"),
        }
    }

    /// Delete the character after the cursor.
    ///
    /// @implements REQ-ACT.edits_are_commands
    pub fn delete_forward(&mut self) {
        let Some(file) = self.editing() else { return };
        let at = self.offset;
        let Some(deleted) = self.buffer().text.chars().nth(at) else { return };
        self.change(Command::Delete { file, offset: at, deleted: deleted.to_string() });
        self.offset = at;
    }

    /// A wheel turned over a pane: scroll it, and take the cursor along so the
    /// next draw does not scroll straight back to it.
    pub fn scroll(&mut self, pane: &str, lines: i64) {
        self.focus_pane(pane);
        let count = self.buffer().text.split('\n').count();
        let down = lines > 0;
        for _ in 0..lines.unsigned_abs() {
            self.step_line(down);
        }
        let top = self.top as i64 + lines;
        self.top = top.clamp(0, count.saturating_sub(1) as i64) as usize;
    }

    /// What the file being edited declares whose name holds each word typed,
    /// in order, as (`path:line` to open, `name  :line`): Ctrl+Shift+O.
    pub fn symbols(&self, query: &str) -> Vec<(String, String)> {
        let Some(path) = self.editing() else { return Vec::new() };
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        self.buffer()
            .text
            .split('\n')
            .enumerate()
            .filter_map(|(n, line)| {
                let name = tracelean_core::surface::definition::declared_on(line)?;
                let lower = name.to_lowercase();
                words
                    .iter()
                    .all(|word| lower.contains(word.as_str()))
                    .then(|| (format!("{path}:{}", n + 1), format!("{name}  :{}", n + 1)))
            })
            .collect()
    }

    /// Every action the keymap binds whose group, words or keys hold each
    /// word typed, as (action, `group: what it is called — its keys`): the
    /// palette.
    pub fn palette(&self, query: &str) -> Vec<(String, String)> {
        let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
        tracelean_core::surface::offer::described(&self.keymap)
            .into_iter()
            .map(|(action, said)| {
                let group = action.split('.').next().unwrap_or_default().to_string();
                (action, format!("{group}: {said}"))
            })
            .filter(|(_, said)| {
                let haystack = said.to_lowercase();
                words.iter().all(|word| haystack.contains(word.as_str()))
            })
            .collect()
    }

    /// An action from a row of the menu bar (or the palette), performed where
    /// the cursor is; the menu, and any list `.` put up, goes away.
    pub fn act_here(&mut self, action: &str) {
        self.offering = None;
        let intent = dispatch(action.to_string(), self.focus(), self.workspace());
        self.perform(intent);
        self.mode = self.keymap.root.clone();
        if self.offering.is_none() {
            self.menu = None;
        }
    }

    /// Offer what can be done where the cursor is, as a menu whose keys are
    /// `1`–`9` then `a`–`z`: the terminal's right-click.
    fn offer_here(&mut self) {
        let pane = self.screen.focus.clone();
        let mut offered = self.offers_at(&pane, self.window_offset(&pane, self.offset));
        offered.truncate(OFFER_KEYS.len());
        let entries = offered
            .iter()
            .zip(OFFER_KEYS)
            .map(|(offer, key)| make::MenuEntry {
                key: key.to_string(),
                description: match &offer.keys {
                    Some(keys) => format!("{}  ({keys})", offer.label),
                    None => offer.label.clone(),
                },
                action: Some(offer.action.clone()),
            })
            .collect();
        self.menu = Some(make::menu_buffer("offers".into(), entries));
        self.offering = Some((pane, self.offset, offered));
        self.say("here", "a key picks one; Escape closes the list");
    }

    /// The key pressed while `.`'s list is up.
    fn pick_offer(&mut self, key: &str) {
        let Some((pane, offset, offered)) = self.offering.take() else { return };
        self.menu = None;
        let Some(index) = OFFER_KEYS.iter().position(|k| *k == key) else {
            self.say("closed", "nothing was picked");
            return;
        };
        let Some(offer) = offered.get(index).cloned() else {
            self.say("nothing there", &format!("`{key}` names no entry"));
            return;
        };
        if let Some(question) = &offer.asks {
            self.say("needs an answer", &format!("{question}: use the window, or the keys {}", offer.keys.unwrap_or_default()));
            return;
        }
        let at = self.window_offset(&pane, offset);
        self.choose(&pane, at, &offer.action, offer.target, None);
    }

    /// A position in a pane's whole buffer as a position in what it shows —
    /// the inverse of `absolute`.
    fn window_offset(&self, pane: &str, offset: usize) -> usize {
        let Some(buffer) = self.buffer_in(pane) else { return offset };
        let top = self.top_of(pane);
        let lines = tracelean_core::surface::view::plain_text(buffer);
        let shift: usize = lines.iter().take(top).map(|l| l.chars().count() + 1).sum();
        offset.saturating_sub(shift)
    }

    /// Every line of the project holding `needle` — as a whole word when
    /// `whole`, a name's uses; ignoring case otherwise, a search — shown as
    /// links to each place.
    pub fn list_occurrences(&mut self, needle: &str, whole: bool) {
        use tracelean_core::surface::findings_view::{findings_view, Found};
        let workspace = self.workspace();
        let found = tracelean_core::surface::definition::occurrences(workspace.files.iter(), needle, whole, 500);
        let rows: Vec<Found> = found
            .iter()
            .map(|d| Found { kind: "at".into(), file: d.path.clone(), line: d.line, message: d.text.clone() })
            .collect();
        let title = if whole { format!("uses of {needle}") } else { format!("search {needle}") };
        let count = rows.len();
        self.show(findings_view(&title, &rows));
        let more = if count >= 500 { " (the first 500)" } else { "" };
        self.say("found", &format!("{count} lines{more} hold {needle}"));
    }

    /// Go to where the name under the cursor is declared.
    fn go_to_definition(&mut self) {
        use tracelean_core::surface::definition::{declarations, identifier_at};
        // A requirement name is defined where its document says it: at the
        // clause's own line, or the top for the requirement.
        let buffer = self.buffer();
        let named = buffer
            .spans
            .iter()
            .find(|s| s.role == tracelean_core::surface::view::Role::Requirement && s.start <= self.offset && self.offset < s.stop)
            .map(|s| buffer.text.chars().skip(s.start).take(s.stop - s.start).collect::<String>());
        if let Some(name) = named {
            let (id, clause) = match name.split_once('.') {
                Some((id, clause)) => (id.to_string(), Some(clause.to_string())),
                None => (name.clone(), None),
            };
            if let Some(file) = self.index().requirements.get(&id).map(|r| r.file.clone()) {
                let text = self.workspace().files.get(&file).cloned().unwrap_or_default();
                let line = clause
                    .and_then(|c| text.lines().position(|l| l.trim_start().starts_with(&format!("{c}:"))))
                    .map_or(1, |n| n + 1);
                let link = requirement_view::link_text(&file, line as u32);
                self.perform(Intent::Display { what: BufferKind::File { path: link } });
                self.say("definition", &format!("{name}: {file}:{line}"));
                return;
            }
        }
        let Some(name) = identifier_at(&self.buffer().text, self.offset) else {
            self.say("no name here", "put the cursor on a name");
            return;
        };
        let workspace = self.workspace();
        let found = declarations(workspace.files.iter(), &name);
        match found.as_slice() {
            [] => self.say("not found", &format!("nothing here declares {name}")),
            [one] => {
                let link = requirement_view::link_text(&one.path, one.line);
                self.perform(Intent::Display { what: BufferKind::File { path: link } });
                self.say("definition", &format!("{name}: {}:{}", one.path, one.line));
            }
            many => {
                let rows: Vec<tracelean_core::surface::findings_view::Found> = many
                    .iter()
                    .map(|d| tracelean_core::surface::findings_view::Found {
                        kind: "declared".into(),
                        file: d.path.clone(),
                        line: d.line,
                        message: d.text.clone(),
                    })
                    .collect();
                let title = format!("definitions of {name}");
                self.show(tracelean_core::surface::findings_view::findings_view(&title, &rows));
                self.say("definitions", &format!("{} places declare {name}", many.len()));
            }
        }
    }

    /// The trace index of the tree on disk, read once and kept.
    fn index(&self) -> std::sync::Arc<tracelean_core::trace::index::Index> {
        if let Some(held) = self.index_cache.borrow().as_ref() {
            return held.clone();
        }
        let fresh = std::sync::Arc::new(tracelean_core::trace::index::build(&self.root));
        *self.index_cache.borrow_mut() = Some(fresh.clone());
        fresh
    }

    /// The disk changed under the index: read it again next time.
    fn forget_index(&self) {
        *self.index_cache.borrow_mut() = None;
        self.problems_stale.set(true);
    }

    /// How many findings the tree on disk has, once counted.
    pub fn problems(&self) -> Option<usize> {
        self.problems.get()
    }

    /// Every key there is (F1): the chords every editor has, then each action
    /// the keymap binds, with the keys that reach it.
    fn keys(&self) -> Vec<Event> {
        let row = |keys: &str, what: &str| Event { kind: keys.to_string(), text: what.to_string() };
        let mut rows: Vec<Event> = CHORDS.iter().map(|(keys, what)| row(keys, what)).collect();
        for (_, said) in tracelean_core::surface::offer::described(&self.keymap) {
            match said.split_once(" — ") {
                Some((what, keys)) => rows.push(row(keys, what)),
                None => rows.push(row("", &said)),
            }
        }
        rows
    }

    /// The bracket beside the cursor and the one that matches it, as offsets
    /// into the focused pane's window — for a frontend to mark both. None away
    /// from a bracket, outside a file, or when the match is not shown.
    pub fn bracket_pair(&self) -> Option<(usize, usize)> {
        if self.editing().is_none() {
            return None;
        }
        let text: Vec<char> = self.buffer().text.chars().collect();
        let pairs = [('(', ')'), ('[', ']'), ('{', '}')];
        let at = [self.offset, self.offset.wrapping_sub(1)]
            .into_iter()
            .find(|i| text.get(*i).is_some_and(|c| pairs.iter().any(|(o, s)| c == o || c == s)))?;
        let c = text[at];
        let (forward, open, shut) = match pairs.iter().find(|(o, s)| *o == c || *s == c) {
            Some((o, s)) => (*o == c, *o, *s),
            None => return None,
        };
        let mut depth = 0usize;
        let mut i = at;
        let other = loop {
            let here = text[i];
            if here == open || here == shut {
                let deeper = (here == open) == forward;
                if deeper {
                    depth += 1;
                } else {
                    depth -= 1;
                    if depth == 0 {
                        break i;
                    }
                }
            }
            if forward {
                i += 1;
                if i >= text.len() {
                    return None;
                }
            } else {
                if i == 0 {
                    return None;
                }
                i -= 1;
            }
        };
        let pane = self.screen.focus.clone();
        let shift = self.offset - self.window_offset(&pane, self.offset);
        (at >= shift && other >= shift).then(|| (at - shift, other - shift))
    }

    /// What a requirement name says, for a pointer resting on it: the clause's
    /// own sentence, or the requirement's title. None for a name no
    /// requirement has.
    pub fn requirement_text(&self, name: &str) -> Option<String> {
        let (id, clause) = match name.split_once('.') {
            Some((id, clause)) => (id, Some(clause)),
            None => (name, None),
        };
        let index = self.index();
        let requirement = index.requirements.get(id)?;
        Some(match clause.and_then(|c| requirement.clauses.get(c)) {
            Some(said) => format!("{name}: {said}"),
            None => format!("{id}: {}", requirement.title),
        })
    }

    /// The files whose paths best match some typed letters, best first.
    ///
    /// `name:42` answers each match at line 42, and `:42` the file in front
    /// at line 42 — `path:line` links, which open there.
    pub fn quick_open(&self, query: &str) -> Vec<String> {
        let (query, line) = match query.rsplit_once(':') {
            Some((name, n)) if n.parse::<usize>().is_ok() => (name, Some(n)),
            _ => (query, None),
        };
        let at = |path: String| match line {
            Some(n) => format!("{path}:{n}"),
            None => path,
        };
        if query.is_empty() && line.is_some() {
            return self.recall().shown.into_iter().map(at).collect();
        }
        // Nothing typed: the files most recently left, then the other tabs.
        if query.is_empty() {
            let here = self.editing();
            let mut recent: Vec<String> = Vec::new();
            let opened = self.screen.opened.iter().rev().filter_map(|b| match &b.kind {
                BufferKind::File { path } => Some(path.clone()),
                _ => None,
            });
            for path in self.back.iter().rev().map(|(path, _)| path.clone()).chain(opened) {
                if Some(&path) != here.as_ref() && !recent.contains(&path) {
                    recent.push(path);
                }
            }
            if !recent.is_empty() {
                recent.truncate(12);
                return recent;
            }
        }
        let files: Vec<String> = self.workspace().files.keys().cloned().collect();
        tracelean_core::surface::quick::matches(&files, query, 12).into_iter().map(at).collect()
    }

    /// Delete what lies between two positions of a pane — a selection made
    /// with the pointer — as one change, and leave the cursor where it began.
    ///
    /// @implements REQ-ACT.edits_are_commands
    pub fn cut(&mut self, pane: &str, start: usize, end: usize) {
        let (from, to) = {
            let (a, b) = (self.absolute(pane, start), self.absolute(pane, end));
            (a.min(b), a.max(b))
        };
        self.focus_pane(pane);
        let Some(file) = self.editing() else {
            self.say("not a file", "only a file's text can be cut");
            return;
        };
        if from == to {
            return;
        }
        let deleted: String = self.buffer().text.chars().skip(from).take(to - from).collect();
        self.change(Command::Delete { file, offset: from, deleted });
        self.offset = from;
        self.mode = INSERT.to_string();
    }

    /// Tab, Shift+Tab or Ctrl+/ over a selection: indent, dedent, or comment
    /// every line it touches, as one change. Blank lines are left alone.
    pub fn over_lines(&mut self, pane: &str, start: usize, end: usize, chord: &str) {
        let (from, to) = {
            let (a, b) = (self.absolute(pane, start), self.absolute(pane, end));
            (a.min(b), a.max(b))
        };
        self.focus_pane(pane);
        self.over_range(from, to, chord);
    }

    /// `over_lines`, between two offsets of the focused buffer. A selection
    /// over those lines stays selected.
    fn over_range(&mut self, from: usize, to: usize, chord: &str) {
        let Some(file) = self.editing() else {
            self.say("not a file", "only a file's lines can be indented");
            return;
        };
        let text = self.buffer().text;
        let step = self.indent_step();
        if chord == "C-/" && comment_marker(&file).is_none() {
            self.say("no line comment", &format!("{file} has none"));
            return;
        }
        let marker = comment_marker(&file).unwrap_or("//");
        // Each touched line: where it starts, and its text.
        let mut lines = Vec::new();
        let mut at = 0;
        for line in text.split('\n') {
            let length = line.chars().count();
            let touched = at <= to && from <= at + length && !(at == to && to > from);
            if touched && !line.trim().is_empty() {
                lines.push((at, line.to_string()));
            }
            at += length + 1;
        }
        let indent = |line: &str| line.chars().take_while(|c| *c == ' ').count();
        let all_commented = lines.iter().all(|(_, l)| l.trim_start().starts_with(marker));
        let least = lines.iter().map(|(_, l)| indent(l)).min().unwrap_or(0);
        // Bottom up, so each command's offset is still where it was read.
        let mut commands = Vec::new();
        for (start, line) in lines.iter().rev() {
            let spaces = indent(line);
            let command = match chord {
                "Tab" => Command::Insert { file: file.clone(), offset: *start, text: " ".repeat(step) },
                "S-Tab" if spaces > 0 => {
                    Command::Delete { file: file.clone(), offset: *start, deleted: " ".repeat((spaces - 1) % step + 1) }
                }
                "C-/" if all_commented => {
                    let rest = &line[spaces + marker.len()..];
                    let width = marker.len() + usize::from(rest.starts_with(' '));
                    Command::Delete { file: file.clone(), offset: start + spaces, deleted: line[spaces..spaces + width].to_string() }
                }
                "C-/" => Command::Insert { file: file.clone(), offset: start + least, text: format!("{marker} ") },
                _ => continue,
            };
            commands.push(command);
        }
        if commands.is_empty() {
            return;
        }
        let count = commands.len();
        // How many line breaks lie between the first and last line changed,
        // read before the change moves them.
        let first = lines.first().map(|(at, _)| *at).unwrap_or(0);
        let spanned = match lines.last() {
            Some((last, _)) => text.chars().skip(first).take(last - first).filter(|c| *c == '\n').count(),
            None => 0,
        };
        self.change(Command::Batch { commands });
        self.offset = self.offset.min(self.buffer().text.chars().count());
        // A selection keeps covering the lines it covered: from the first
        // one's start to the last one's end.
        if self.anchor.is_some() {
            let now: Vec<char> = self.buffer().text.chars().collect();
            let mut end = first;
            let mut seen = 0;
            while end < now.len() && !(seen == spanned && now[end] == '\n') {
                seen += usize::from(now[end] == '\n');
                end += 1;
            }
            self.anchor = Some(first);
            self.offset = end;
        }
        self.say("changed", &format!("{count} line(s)"));
    }

    /// Text from the clipboard, typed in one change.
    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.take_selection();
        self.insert(text);
    }

    /// What is selected, start before end, when anything is.
    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let length = self.buffer().text.chars().count();
        let (a, b) = (anchor.min(length), self.offset.min(length));
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    /// The selected text.
    pub fn selected(&self) -> Option<String> {
        let (from, to) = self.selection()?;
        Some(self.buffer().text.chars().skip(from).take(to - from).collect())
    }

    /// The selection as offsets into the focused pane's window, for a
    /// frontend to mark; the part above the window is cut off.
    pub fn selection_shown(&self) -> Option<(usize, usize)> {
        let (from, to) = self.selection()?;
        let pane = self.screen.focus.clone();
        let shift = self.offset - self.window_offset(&pane, self.offset);
        (to > shift).then(|| (from.saturating_sub(shift), to - shift))
    }

    /// Where the name under the cursor is written, whole, in the focused
    /// file's window: its length and each start as an offset into the window.
    /// Nothing while something is selected, or for a name of one letter.
    pub fn occurrences_shown(&self) -> Option<(usize, Vec<usize>)> {
        if self.selection().is_some() {
            return None;
        }
        self.editing()?;
        let chars: Vec<char> = self.buffer().text.chars().collect();
        let part = |c: char| c.is_alphanumeric() || c == '_';
        let at = self.offset.min(chars.len());
        let start = at - chars[..at].iter().rev().take_while(|c| part(**c)).count();
        let end = at + chars[at..].iter().take_while(|c| part(**c)).count();
        if end - start < 2 {
            return None;
        }
        let word = &chars[start..end];
        let pane = self.screen.focus.clone();
        let shift = self.offset - self.window_offset(&pane, self.offset);
        let mut lines = 0;
        let mut stop = shift;
        while stop < chars.len() && lines < self.region.height as usize {
            if chars[stop] == '\n' {
                lines += 1;
            }
            stop += 1;
        }
        let found = (shift..stop.saturating_sub(word.len() - 1))
            .filter(|&i| {
                chars[i..].starts_with(word)
                    && (i == 0 || !part(chars[i - 1]))
                    && chars.get(i + word.len()).is_none_or(|c| !part(*c))
            })
            .map(|i| i - shift)
            .collect();
        Some((word.len(), found))
    }

    /// Select between two positions of a pane — a drag with the pointer —
    /// the cursor at `end`.
    pub fn select(&mut self, pane: &str, start: usize, end: usize) {
        self.place(pane, end);
        self.anchor = Some(self.absolute(pane, start));
    }

    /// Remove what is selected from a file, as one change, leaving the cursor
    /// where it began. Answers whether there was anything to remove.
    fn take_selection(&mut self) -> bool {
        let (Some((from, to)), Some(file)) = (self.selection(), self.editing()) else {
            self.anchor = None;
            return false;
        };
        let deleted: String = self.buffer().text.chars().skip(from).take(to - from).collect();
        self.change(Command::Delete { file, offset: from, deleted });
        self.offset = from;
        self.anchor = None;
        true
    }

    /// One key, interpreted.
    ///
    /// Here rather than in a frontend because a key means the same thing in a
    /// terminal and in a window, and two implementations of that would be two
    /// answers to what the editor does.
    ///
    /// @implements REQ-ACT.one_path
    pub fn key(&mut self, key: &str) {
        if self.offering.is_some() {
            self.pick_offer(key);
            return;
        }
        match tracelean_core::surface::keymap::step(
            self.keymap.clone(),
            self.mode.clone(),
            key.to_string(),
        ) {
            Outcome::Enter { mode } if mode == INSERT => {
                // Insert mode has no parent, so an unbound key is text rather
                // than a way back to the root. Escape is bound, which is how it
                // is still possible to leave.
                if self.editing().is_none() {
                    self.say("not a file", "there is nothing here to type into");
                } else {
                    self.mode = mode;
                    self.menu = None;
                    self.say("insert", "Escape leaves");
                }
            }
            Outcome::Enter { mode } if mode == self.keymap.root => {
                // Back to where keys are commands. There is no menu for the
                // root mode: the root mode is the editor, not a list.
                self.mode = mode;
                self.menu = None;
                self.say("normal", ". lists what can be done here, Space opens the leader menu");
            }
            Outcome::Enter { mode } => {
                // Entering a mode offers what is in it, which is the menu the
                // core produces for that mode — beside the buffer, so the next
                // key is still about whatever the cursor was on.
                self.mode = mode.clone();
                self.menu = Some(self.produce(BufferKind::Menu { title: mode }));
            }
            Outcome::Leave { mode } => {
                self.mode = mode.clone();
                self.menu = if mode == self.keymap.root {
                    None
                } else {
                    Some(self.produce(BufferKind::Menu { title: mode }))
                };
                self.say("left", "back one mode");
            }
            Outcome::Dispatch { action } => {
                let intent = dispatch(action, self.focus(), self.workspace());
                self.perform(intent);
                self.mode = self.keymap.root.clone();
                // Unless what it did was put up a list to pick from.
                if self.offering.is_none() {
                    self.menu = None;
                }
            }
            Outcome::PassThrough => self.typed(key),
        }
    }

    /// Where the word before (or after) the cursor begins (or ends): past any
    /// spaces and punctuation first, then past the word — Ctrl+Left and
    /// Ctrl+Right in every editor.
    fn word_edge(&self, forward: bool) -> usize {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let word = |c: char| c.is_alphanumeric() || c == '_';
        let mut at = self.offset.min(text.len());
        if forward {
            while at < text.len() && !word(text[at]) {
                at += 1;
            }
            while at < text.len() && word(text[at]) {
                at += 1;
            }
        } else {
            while at > 0 && !word(text[at - 1]) {
                at -= 1;
            }
            while at > 0 && word(text[at - 1]) {
                at -= 1;
            }
        }
        at
    }

    /// Delete from the cursor to `edge`, either side of it, as one change.
    fn delete_to(&mut self, edge: usize) {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return;
        };
        let (from, to) = (self.offset.min(edge), self.offset.max(edge));
        if from == to {
            return;
        }
        let deleted: String = self.buffer().text.chars().skip(from).take(to - from).collect();
        self.change(Command::Delete { file, offset: from, deleted });
        self.offset = from;
    }

    /// The cursor's line: where it starts, and its text without the line
    /// break. None outside a file.
    fn this_line(&mut self) -> Option<(String, usize, String)> {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return None;
        };
        let text: Vec<char> = self.buffer().text.chars().collect();
        let (_, column) = self.line_and_column();
        let start = self.offset - column;
        let line: String = text[start..].iter().take_while(|c| **c != '\n').collect();
        Some((file, start, line))
    }

    /// Alt+Up and Alt+Down: swap the cursor's line with the one above or
    /// below, the cursor going with it.
    fn move_line(&mut self, down: bool) {
        let Some((file, start, line)) = self.this_line() else { return };
        let text: Vec<char> = self.buffer().text.chars().collect();
        let end = start + line.chars().count();
        let (from, first, second) = if down {
            if end >= text.len() {
                return;
            }
            let below: String = text[end + 1..].iter().take_while(|c| **c != '\n').collect();
            (start, line.clone(), below)
        } else {
            if start == 0 {
                return;
            }
            let begins = text[..start - 1].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
            let above: String = text[begins..start - 1].iter().collect();
            (begins, above, line.clone())
        };
        let column = self.offset - start;
        self.change(Command::Batch {
            commands: vec![
                Command::Delete { file: file.clone(), offset: from, deleted: format!("{first}\n{second}") },
                Command::Insert { file, offset: from, text: format!("{second}\n{first}") },
            ],
        });
        self.offset = if down { from + second.chars().count() + 1 + column } else { from + column };
    }

    /// Shift+Alt+Down: a copy of the cursor's line under it, the cursor
    /// going to the copy.
    fn duplicate_line(&mut self) {
        let Some((file, start, line)) = self.this_line() else { return };
        let end = start + line.chars().count();
        let length = line.chars().count();
        self.change(Command::Insert { file, offset: end, text: format!("\n{line}") });
        self.offset += length + 1;
    }

    /// Ctrl+Shift+K: remove the cursor's line.
    fn delete_line(&mut self) {
        let Some((file, start, line)) = self.this_line() else { return };
        let length = self.buffer().text.chars().count();
        let end = start + line.chars().count();
        let (from, deleted) = if end < length {
            (start, format!("{line}\n"))
        } else if start > 0 {
            (start - 1, format!("\n{line}"))
        } else {
            (start, line)
        };
        if deleted.is_empty() {
            return;
        }
        self.change(Command::Delete { file, offset: from, deleted });
        self.offset = from.min(self.buffer().text.chars().count());
    }

    /// Shift+Tab: take one level of indentation off the cursor's line.
    fn dedent_line(&mut self) {
        let Some((file, start, line)) = self.this_line() else { return };
        let spaces = line.chars().take_while(|c| *c == ' ').count();
        let step = self.indent_step();
        let width = match spaces {
            0 => return,
            n => (n - 1) % step + 1,
        };
        self.change(Command::Delete { file, offset: start, deleted: " ".repeat(width) });
        self.offset = self.offset.saturating_sub(width).max(start);
    }

    /// Ctrl+/: comment the cursor's line out, or back in, with the file's own
    /// line comment.
    fn toggle_comment(&mut self) {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return;
        };
        let Some(marker) = comment_marker(&file) else {
            self.say("no line comment", &format!("{file} has none"));
            return;
        };
        let text: Vec<char> = self.buffer().text.chars().collect();
        let (_, column) = self.line_and_column();
        let start = self.offset - column;
        let line: String = text[start..].iter().take_while(|c| **c != '\n').collect();
        let indent = line.chars().take_while(|c| c.is_whitespace()).count();
        let rest: String = line.chars().skip(indent).collect();
        let at = start + indent;
        if rest.starts_with(marker) {
            let width = marker.chars().count() + usize::from(rest[marker.len()..].starts_with(' '));
            let deleted: String = rest.chars().take(width).collect();
            self.change(Command::Delete { file, offset: at, deleted });
            self.offset = if self.offset >= at + width { self.offset - width } else { at.min(self.offset) };
        } else {
            let inserted = format!("{marker} ");
            self.change(Command::Insert { file, offset: at, text: inserted.clone() });
            if self.offset >= at {
                self.offset += inserted.chars().count();
            }
        }
    }

    /// How deep one level of indentation is in the file being edited: Lean's
    /// two spaces, four elsewhere.
    fn indent_step(&self) -> usize {
        match self.editing() {
            Some(path) if path.ends_with(".lean") => 2,
            _ => 4,
        }
    }

    /// The cursor's line up to the cursor.
    fn before_cursor(&self) -> String {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let at = self.offset.min(text.len());
        let start = text[..at].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        text[start..at].iter().collect()
    }

    /// The indentation the cursor's line starts with.
    fn indent_here(&self) -> String {
        self.before_cursor().chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    /// Whether the cursor sits between a bracket and its closer, `(|)`.
    fn inside_pair(&self) -> bool {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let (before, after) = (self.offset.checked_sub(1).and_then(|i| text.get(i)), text.get(self.offset));
        matches!((before, after), (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}')))
    }

    /// Whether the cursor is between two `"` with nothing between them.
    fn inside_quotes(&self) -> bool {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let before = self.offset.checked_sub(1).and_then(|i| text.get(i));
        before == Some(&'"') && text.get(self.offset) == Some(&'"')
    }

    /// One character typed. An opening bracket brings its closer when
    /// nothing but space or another closer follows; a closer typed where the
    /// same closer already is steps over it. In code (not prose), `"` does
    /// the same, unless it ends a word.
    fn type_char(&mut self, typed: &str) {
        let c = typed.chars().next().unwrap_or(' ');
        let next = self.buffer().text.chars().nth(self.offset);
        let prose = self.editing().is_none_or(|path| path.ends_with(".md") || path.ends_with(".txt"));
        if c == '"' && !prose {
            if next == Some('"') {
                self.offset += 1;
                return;
            }
            let previous = self.offset.checked_sub(1).and_then(|i| self.buffer().text.chars().nth(i));
            let free = next.is_none_or(|n| n.is_whitespace() || ")]};,".contains(n));
            if free && previous.is_none_or(|p| !p.is_alphanumeric() && !"\"\\".contains(p)) {
                self.insert("\"\"");
                self.offset -= 1;
                return;
            }
            return self.insert(typed);
        }
        if matches!(c, ')' | ']' | '}') && next == Some(c) && self.editing().is_some() {
            self.offset += 1;
            return;
        }
        let close = match c {
            '(' => ')',
            '[' => ']',
            '{' => '}',
            _ => return self.insert(typed),
        };
        let free = next.is_none_or(|n| n.is_whitespace() || ")]};,".contains(n));
        if free && self.editing().is_some() {
            self.insert(&format!("{c}{close}"));
            self.offset -= 1;
        } else {
            self.insert(typed);
        }
    }

    /// What Enter types: a line break, then the indentation of the line it
    /// breaks — one level deeper when that line opens a block (a bracket, `:`,
    /// `:=`, `by`, `where`, `do`).
    fn line_break(&self) -> String {
        let before = self.before_cursor();
        let indent = self.indent_here();
        let end = before.trim_end();
        let opens = matches!(end.chars().last(), Some('{' | '(' | '[' | ':'))
            || [":=", " by", " where", " do"].iter().any(|word| end.ends_with(word));
        let deeper = if opens { " ".repeat(self.indent_step()) } else { String::new() };
        format!("\n{indent}{deeper}")
    }

    /// A key no binding claimed. In insert mode that is text; anywhere else it
    /// is a key that does nothing, and saying so beats silence.
    fn typed(&mut self, key: &str) {
        if self.mode != INSERT {
            self.anchor = None;
            match key {
                "Left" | "Right" if self.tree_step(key == "Right") => {}
                "Left" => self.step(false),
                "Right" => self.step(true),
                "Up" => self.step_line(false),
                "Down" => self.step_line(true),
                // What a click on the same place does: open the file, the
                // folder, the link the cursor is on.
                "Enter" => match self.offered().first() {
                    Some(action) => {
                        let pane = self.screen.focus.clone();
                        let at = self.window_offset(&pane, self.offset);
                        self.choose(&pane, at, &action.clone(), None, None);
                    }
                    None => self.say("nothing happened", "nothing here opens; . lists what can be done here"),
                },
                _ => self.say("nothing happened", &format!("`{key}` is not bound here")),
            }
            return;
        }
        // A selection: Tab indents the lines it spans, Backspace removes it,
        // and what is typed replaces it; a plain arrow lets it go.
        if let Some((from, to)) = self.selection() {
            let text: String = self.buffer().text.chars().skip(from).take(to - from).collect();
            match key {
                "Tab" if text.contains('\n') => return self.over_range(from, to, "Tab"),
                "Backspace" => {
                    self.take_selection();
                    return;
                }
                "Left" | "Right" | "Up" | "Down" => self.anchor = None,
                _ if key == "Enter" || key == "Tab" || key.chars().count() == 1 => {
                    self.take_selection();
                }
                _ => {}
            }
        }
        match key {
            "Enter" => {
                // Between a pair just opened, the closer goes a line further
                // down and the cursor sits indented between them.
                let broken = self.line_break();
                if self.inside_pair() {
                    let closing = format!("\n{}", self.indent_here());
                    self.insert(&format!("{broken}{closing}"));
                    self.offset -= closing.chars().count();
                } else {
                    self.insert(&broken);
                }
            }
            "Tab" => {
                let step = self.indent_step();
                let (_, column) = self.line_and_column();
                self.insert(&" ".repeat(step - column % step));
            }
            "Backspace" => {
                if self.inside_pair() || self.inside_quotes() {
                    self.offset += 1;
                    self.delete_back();
                }
                self.delete_back();
            }
            "Left" => self.step(false),
            "Right" => self.step(true),
            "Up" => self.step_line(false),
            "Down" => self.step_line(true),
            other if other.chars().count() == 1 => self.type_char(other),
            other => self.say("nothing happened", &format!("`{other}` is not text")),
        }
    }

    /// Carry out an intent. Every branch either shows a buffer the core made or
    /// changes the workspace through a command.
    ///
    /// @implements REQ-ACT.action_to_intent
    pub fn perform(&mut self, intent: Intent) {
        match intent {
            // An absolute folder is another project: open it as the tree.
            Intent::Display { what: BufferKind::File { path } }
                if Path::new(&path).is_absolute() && Path::new(&path).is_dir() =>
            {
                self.reopen(PathBuf::from(path));
            }
            // A `path:line` link opens the file at that line.
            Intent::Display { what: BufferKind::File { path } }
                if !self.workspace().files.contains_key(&path)
                    && requirement_view::split_link(&path).1.is_some() =>
            {
                let (file, line) = requirement_view::split_link(&path);
                let (file, line) = (file.to_string(), line.unwrap_or(1));
                self.mark_jump();
                self.perform(Intent::Display { what: BufferKind::File { path: file } });
                self.go_to_line(line as usize);
            }
            // Where the name under the cursor is declared: there, when it is
            // one place, else a list of the places.
            Intent::Display { what: BufferKind::Record { title } } if title == "definition" => {
                self.go_to_definition();
            }
            Intent::Display { what: BufferKind::Record { title } } if title == "references" => {
                use tracelean_core::surface::definition::identifier_at;
                match identifier_at(&self.buffer().text, self.offset) {
                    Some(name) => self.list_occurrences(&name, true),
                    None => self.say("no name here", "put the cursor on a name"),
                }
            }
            // The list of what can be done here goes where a mode's menu goes,
            // beside the buffer, and the next key picks from it.
            Intent::Display { what: BufferKind::Menu { title } } if title == "offers" => {
                self.offer_here();
            }
            // Opening a folder opens or closes it in the tree.
            Intent::Display { what: BufferKind::File { path } } if self.is_folder(&path) => {
                self.toggle_folder(&path);
            }
            Intent::Display { what } => {
                // A file shown is a file the tree can show: open what holds it.
                if let BufferKind::File { path } = &what {
                    if self.workspace().files.contains_key(path) {
                        self.open_folders.extend(explorer::holders(path));
                    }
                }
                let buffer = self.produce(what);
                self.show(buffer);
                if self.screen.opened.iter().any(|b| self.is_tree(b)) {
                    self.refresh_views();
                }
            }
            Intent::Edit { command } => {
                // A file made or renamed is shown where it now is, and a file
                // just made is opened to be written.
                let made = match &command {
                    Command::CreateFile { path } => Some(path.clone()),
                    Command::Batch { commands } => commands.iter().find_map(|c| match c {
                        Command::CreateFile { path } => Some(path.clone()),
                        _ => None,
                    }),
                    _ => None,
                };
                if let Command::RenameFile { to, .. } = &command {
                    self.open_folders.extend(explorer::holders(to));
                }
                if let Some(path) = &made {
                    self.open_folders.extend(explorer::holders(path));
                }
                self.edit(command);
                if let Some(path) = made.filter(|p| self.workspace().files.contains_key(p)) {
                    self.perform(Intent::Display { what: BufferKind::File { path } });
                }
            }
            Intent::Travel { move_ } => self.travel(move_),
            Intent::Observe { watch } => self.watch(watch),
            Intent::Arrange { how } => self.rearrange(how),
            Intent::Persist => self.persist(),
            Intent::Refuse { why } => self.refuse(why),
        }
    }

    /// Change the arrangement, and keep each pane's cursor where it was.
    ///
    /// The change itself is the core's — one function, so a key and a pointer
    /// make the same change (`REQ-SCREEN.one_arrangement_path`). What is left
    /// here is the shell's part: the cursor and the first shown line belong to
    /// the pane the focus is in, so leaving a pane parks them and arriving at
    /// one takes them back out.
    fn rearrange(&mut self, how: Arrangement) {
        let was = self.screen.focus.clone();
        self.screen = arrange(how, self.region, self.screen.clone());
        if self.screen.focus != was {
            self.parked.insert(was, (self.offset, self.top));
            let (offset, top) = self.parked.remove(&self.screen.focus).unwrap_or((0, 0));
            self.offset = offset;
            self.top = top;
        }
        // Closing and showing both change what the focused pane holds, and a
        // cursor past the end of the new buffer is a cursor nothing can draw.
        let length = self.buffer().text.chars().count();
        self.offset = self.offset.min(length);
        // A pane the layout no longer places has no cursor to remember.
        let placed: Vec<String> =
            panes(&self.screen.layout).into_iter().map(|(pane, _)| pane).collect();
        self.parked.retain(|pane, _| placed.contains(pane));
    }

    /// Ask the core for the buffer a kind names. Nothing is built here.
    fn produce(&mut self, what: BufferKind) -> Buffer {
        match what {
            BufferKind::File { path } => {
                let text = self.workspace().files.get(&path).cloned().unwrap_or_default();
                let marks = marks_in(&path, &text);
                make::file_buffer(path, text, marks)
            }
            BufferKind::Directory { path } => self.listing_of(&path),
            // A change an agent is proposing is reviewed against what the agent
            // wrote; anything else against what was last saved.
            BufferKind::Review { target } => match &self.agent_tree {
                Some(agent) if self.pending.iter().any(|c| touched(c) == target) => {
                    let before = self.workspace().files.get(&target).cloned().unwrap_or_default();
                    let after = agent.files.get(&target).cloned().unwrap_or_default();
                    make::review_buffer(target, before, after)
                }
                _ => {
                    let before = self.saved.files.get(&target).cloned().unwrap_or_default();
                    let after = self.workspace().files.get(&target).cloned().unwrap_or_default();
                    make::review_buffer(target, before, after)
                }
            },
            // The opened set is a menu like any other, and it is the core that
            // makes it: a strip built here would be a second answer to what the
            // session holds, beside the one `screen.opened` already gives.
            BufferKind::Menu { title } if title == "opened" => self.strip(),
            // Two stations' content. Menus rather than records because each row
            // is somewhere to go, which is what a menu is here — the strip and
            // the stations are menus for the same reason.
            BufferKind::Menu { title } if title == "requirements" => {
                let nodes = self.nodes();
                if nodes.is_empty() {
                    // An empty index is the honest answer, and a blank panel
                    // is a bad way to give it: say so, and say how to start.
                    return make::menu_buffer(
                        "requirements".into(),
                        vec![
                            make::MenuEntry {
                                key: "no requirements yet".into(),
                                description: String::new(),
                                action: None,
                            },
                            make::MenuEntry {
                                key: "right-click".into(),
                                description: "New requirement…".into(),
                                action: None,
                            },
                        ],
                    );
                }
                make::requirements_buffer(nodes)
            }
            BufferKind::Menu { title } if title == "welcome" => self.welcome(),
            BufferKind::Menu { title } if title == "design" => {
                make::design_buffer(self.nodes())
            }
            BufferKind::Menu { title } => {
                let entries = self.menu_entries(&title);
                make::menu_buffer(title, entries)
            }
            BufferKind::Record { title } if title == "sandbox" => self.sandbox(),
            BufferKind::Record { title } if title == "history" => {
                use tracelean_core::surface::history_view::history_view;
                history_view(&self.points())
            }
            BufferKind::Record { title } if title == "findings" || title == "check" => {
                // Checking the tree is asking about the disk as it is now,
                // edits made elsewhere included.
                if title == "check" {
                    self.forget_index();
                }
                tracelean_core::surface::findings_view::findings_view(&title, &self.found())
            }
            BufferKind::Record { title } if title.starts_with("judge ") => {
                self.judge(&title["judge ".len()..])
            }
            BufferKind::Record { title } if title.starts_with("context ") => {
                self.agent_context(&title["context ".len()..])
            }
            BufferKind::Record { title } if title == "trace" => self.file_trace(),
            BufferKind::Record { title } if title.starts_with("requirement ") => {
                self.requirement(&title["requirement ".len()..])
            }
            BufferKind::Record { title } if title == "keys" => make::record_buffer(title, self.keys()),
            BufferKind::Record { title } => {
                let events = self.report(&title);
                make::record_buffer(title, events)
            }
        }
    }

    /// Work on another tree: everything this editor holds is about one
    /// project, so it starts again there, keeping only the window's size.
    fn reopen(&mut self, root: PathBuf) {
        if self.workspace() != self.saved {
            self.say("not opened", "save first (Ctrl+S): this project has changes not on disk");
            return;
        }
        let region = self.region;
        let shown = root.display().to_string();
        let tracking = self.recent.is_some();
        *self = Editor::open(root, self.keymap.clone());
        self.region = region;
        if tracking {
            self.track_recent();
        }
        self.say("opened", &shown);
    }

    /// The file the document pane shows and where its cursor is.
    fn document_place(&self) -> Option<(String, usize)> {
        use tracelean_core::surface::screen::DOCUMENT;
        let id = panes(&self.screen.layout).into_iter().find(|(pane, _)| pane == DOCUMENT)?.1;
        let buffer = self.screen.opened.iter().find(|b| b.id == id)?;
        let BufferKind::File { path } = &buffer.kind else { return None };
        let offset = if self.screen.focus == DOCUMENT {
            self.offset
        } else {
            self.parked.get(DOCUMENT).map_or(0, |parked| parked.0)
        };
        Some((path.clone(), offset))
    }

    /// Note where the document is before a jump takes it elsewhere.
    fn mark_jump(&mut self) {
        let Some(place) = self.document_place() else { return };
        if self.back.last() != Some(&place) {
            self.back.push(place);
        }
        if self.back.len() > 100 {
            self.back.remove(0);
        }
        self.forward.clear();
    }

    /// Back to where the document was before the last jump (Alt+Left), or
    /// forward again (Alt+Right).
    fn go_back(&mut self, backwards: bool) {
        let taken = if backwards { self.back.pop() } else { self.forward.pop() };
        let Some((path, offset)) = taken else {
            let way = if backwards { "back" } else { "forward" };
            return self.say("nowhere", &format!("nothing to go {way} to"));
        };
        if let Some(here) = self.document_place() {
            if backwards { self.forward.push(here) } else { self.back.push(here) }
        }
        if !self.workspace().files.contains_key(&path) {
            return self.say("gone", &format!("{path} is no longer here"));
        }
        self.open_folders.extend(explorer::holders(&path));
        let buffer = self.produce(BufferKind::File { path: path.clone() });
        // Going back is not itself a jump to note.
        let kept = (std::mem::take(&mut self.back), std::mem::take(&mut self.forward));
        self.show(buffer);
        (self.back, self.forward) = kept;
        let text = self.buffer().text;
        self.offset = offset.min(text.chars().count());
        let line = text.chars().take(self.offset).filter(|c| *c == '\n').count();
        self.top = line.saturating_sub(3);
        self.refresh_views();
        self.say(if backwards { "back" } else { "forward" }, &format!("{path}:{}", line + 1));
    }

    /// Put the cursor at the start of a one-based line of the focused buffer,
    /// and scroll so a few lines above it show too.
    fn go_to_line(&mut self, line: usize) {
        let text = self.buffer().text;
        let wanted = line.saturating_sub(1);
        let mut offset = 0;
        for (n, row) in text.split('\n').enumerate() {
            if n == wanted {
                break;
            }
            offset += row.chars().count() + 1;
        }
        self.offset = offset.min(text.chars().count());
        // Scrolled only when the line is out of sight, then with a few lines
        // above it.
        let height = (self.region.height as usize).saturating_sub(1).max(1);
        if wanted < self.top || wanted >= self.top + height {
            self.top = wanted.saturating_sub(3);
        }
    }

    /// One requirement, opened: its clauses, the level each reached, and every
    /// annotation that claims it as a link to its line.
    ///
    /// A clause's own name (`REQ-X.clause`) opens the requirement it belongs to.
    fn requirement(&self, named: &str) -> Buffer {
        use tracelean_core::surface::requirement_view::{
            requirement_view, Claim, ClauseShown, RequirementShown,
        };
        let id = named.split('.').next().unwrap_or(named);
        let index = self.index();
        let Some(requirement) = index.requirements.get(id) else {
            return make::record_buffer(
                format!("requirement {named}"),
                vec![Event { kind: "unknown".into(), text: format!("no requirement is called {id}") }],
            );
        };
        let records: Vec<tracelean_core::trace::record::Evidence> = tracelean_core::trace::lockfile::read(&self.root)
            .map(|l| l.evidence)
            .unwrap_or_default();
        let levels = levels_of(&records);
        let clauses = requirement
            .clause_keys()
            .into_iter()
            .map(|clause| {
                let mut claims: Vec<Claim> = index
                    .links
                    .iter()
                    .filter(|link| link.req_id == id && link.clause == clause)
                    .map(|link| Claim {
                        role: link.role.as_str().to_string(),
                        path: link.anchor.file.clone(),
                        line: link.line,
                        symbol: symbol_of(&link.anchor),
                    })
                    .collect();
                claims.sort_by(|a, b| (&a.role, &a.path, a.line).cmp(&(&b.role, &b.path, b.line)));
                ClauseShown {
                    text: clause
                        .as_ref()
                        .and_then(|k| requirement.clauses.get(k).cloned())
                        .unwrap_or_else(|| requirement.title.clone()),
                    level: levels.get(&(id.to_string(), clause.clone())).copied().unwrap_or(Level::L1),
                    chain: chain_of(&records, id, clause.as_deref()),
                    key: clause,
                    claims,
                }
            })
            .collect();
        requirement_view(RequirementShown {
            id: id.to_string(),
            title: requirement.title.clone(),
            file: requirement.file.clone(),
            status: requirement.status.as_str().to_string(),
            refines: requirement.refines.clone(),
            refined_by: index
                .requirements
                .values()
                .filter(|other| other.refines.iter().any(|parent| parent == id))
                .map(|other| other.id.clone())
                .collect(),
            clauses,
            width: self.pane_width(tracelean_core::surface::screen::DOCUMENT),
        })
    }

    /// The prompt for judging a clause against its model, to copy and carry to
    /// a person. Nothing is called (`REQ-JUDGE.no_call`); recording the verdict
    /// is `tracelean-trace --judge <clause> --verdict … --by …`.
    ///
    /// @implements REQ-JUDGE.prompt_exported
    fn judge(&self, name: &str) -> Buffer {
        use tracelean_core::trace::material;
        let (req_id, clause) = match name.split_once('.') {
            Some((req, clause)) => (req, Some(clause)),
            None => (name, None),
        };
        let index = self.index();
        let refuse = |why: String| {
            make::record_buffer(format!("judge {name}"), vec![Event { kind: "cannot judge".into(), text: why }])
        };
        let Some(model_at) = material::model_of(&index, req_id, clause) else {
            return refuse(format!("nothing models {name}, so there is nothing to judge it against"));
        };
        // The model's declaration, as the command line reads it.
        let source = self
            .workspace()
            .files
            .get(&model_at.file)
            .map(|text| {
                text.lines()
                    .skip(model_at.start_line as usize)
                    .take((model_at.end_line.saturating_sub(model_at.start_line) + 1) as usize)
                    .collect::<Vec<_>>()
                    .join("\n")
            })
            .unwrap_or_default();
        match material::assemble(&index, req_id, clause, source, None) {
            Ok((material, _)) => {
                requirement_view::judge_view(name, tracelean_core::judge::prompt(material))
            }
            Err(why) => refuse(format!("cannot assemble the material: {why:?}")),
        }
    }

    /// How many characters a named pane has for a line, or the side panel's
    /// when that pane is not on the screen.
    fn pane_width(&self, pane: &str) -> usize {
        self.laid_out(self.region)
            .iter()
            .find(|p| p.pane == pane)
            .map(|p| (p.at.width as usize).saturating_sub(2))
            .filter(|w| *w > 0)
            .unwrap_or_else(|| self.side_width())
    }

    /// Every requirement, with what its evidence reached.
    ///
    /// Read here and handed to a producer, rather than read by one: a producer
    /// is a function of what it is given and reads no disk
    /// ([ARCH-CORE-SHELL](../../reqs/arch/ARCH-CORE-SHELL.md)). The level is a
    /// requirement's own, aggregated over its clauses by the same minimum the
    /// ladder uses everywhere — a requirement is worth its weakest clause,
    /// because a set of obligations is met only as well as the one least met.
    fn nodes(&self) -> Vec<make::Node> {
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let levels = levels_of(lock.as_ref().map(|l| &l.evidence[..]).unwrap_or(&[]));
        index
            .requirements
            .iter()
            .map(|(id, requirement)| {
                let reached = requirement
                    .clause_keys()
                    .into_iter()
                    .map(|clause| {
                        levels.get(&(id.clone(), clause)).copied().unwrap_or(Level::L1)
                    })
                    .min()
                    .unwrap_or(Level::L1);
                // How many clauses something implements, of how many: the
                // level is a minimum and says little until evidence is
                // recorded, and this says where the work is.
                let clauses = requirement.clause_keys();
                let implemented = clauses
                    .iter()
                    .filter(|clause| {
                        index.links.iter().any(|link| {
                            &link.req_id == id
                                && &link.clause == *clause
                                && link.role == tracelean_core::trace::annotation::Role::Implements
                        })
                    })
                    .count();
                make::Node {
                    id: id.clone(),
                    title: requirement.title.clone(),
                    refines: requirement.refines.clone(),
                    level: reached,
                    implemented,
                    clauses: clauses.len(),
                }
            })
            .collect()
    }

    /// The sandbox station: what the tree changed, what a tool said, and what
    /// its usage is estimated to have cost.
    ///
    /// Nothing here starts anything. The changes are the ones already waiting
    /// to be taken in, the transcript is read from wherever the tool wrote it,
    /// and the price table is this project's own — all three are read by the
    /// shell and handed to `sandbox_buffer`, which is the function that decides
    /// what the buffer says.
    ///
    /// @implements ARCH-NO-DRIVING.no_model_call
    fn sandbox(&self) -> Buffer {
        use tracelean_core::observe::{claude, cost, transcript, watch, workcopy as session};
        use tracelean_core::surface::sandbox_view::{sandbox_view, Pending, SandboxView, Shown};
        let Some(live) = &self.session else {
            return sandbox_view(SandboxView {
                session: None,
                pending: Vec::new(),
                said: Vec::new(),
                spend: cost::Spend::default(),
                width: self.side_width(),
            });
        };
        // The agent's own record: Claude Code's, filed under the project's
        // name since the copy is mounted there; else the generic transcript.
        let text = watch::claude_transcript(&self.root, session::began(live));
        let (said, usage) = if text.is_empty() {
            let read = watch::transcript_of(&self.root);
            (read.events, read.usage)
        } else {
            (claude::events(&text), transcript::read(text).usage)
        };
        let spend = cost::spend_of(watch::table_of(&self.root), usage);
        let mut pending: Vec<Pending> = Vec::new();
        for command in &self.pending {
            let path = touched(command);
            if pending.iter().any(|p| p.path == path) {
                continue;
            }
            let kind = match command {
                Command::CreateFile { .. } => "created",
                Command::DeleteFile { .. } => "deleted",
                _ => "modified",
            };
            // The size of the change, as its diff counts it.
            let lines = |files: Option<&String>| -> Vec<String> {
                files.map(|t| t.lines().map(String::from).collect()).unwrap_or_default()
            };
            let before = lines(self.workspace().files.get(&path));
            let after = lines(self.agent_tree.as_ref().and_then(|t| t.files.get(&path)));
            let diff = make::diff_lines(before, after);
            use tracelean_core::surface::view::Role;
            let count = |role: Role| diff.iter().filter(|d| d.role == role).count();
            let (added, removed) = (count(Role::Added), count(Role::Removed));
            pending.push(Pending { kind: kind.into(), path, added, removed });
        }
        let containment = match tracelean_core::observe::workspace::probe_containment() {
            tracelean_core::observe::Capability::Contained { mechanism } => {
                format!("contained by {mechanism}")
            }
            tracelean_core::observe::Capability::Unavailable { reason } => {
                format!("NOT contained: {reason}. Install bubblewrap for the command below to work.")
            }
        };
        sandbox_view(SandboxView {
            session: Some(Shown {
                id: live.id.clone(),
                containment,
                command: tracelean_core::observe::sandbox::short_command(live),
            }),
            pending,
            said,
            spend,
            width: self.side_width(),
        })
    }

    /// What the document pane shows before a file is opened: the places to
    /// start, each a row to click, each with the keys that reach it — then the
    /// other folders opened before, when the frontend keeps that list.
    fn welcome(&self) -> Buffer {
        let row = |action: &str, description: &str| make::MenuEntry {
            key: tracelean_core::surface::offer::keys_for(&self.keymap, action).unwrap_or_default(),
            description: description.to_string(),
            action: Some(action.to_string()),
        };
        let here = recall::named(&self.root);
        let others: Vec<String> =
            self.recent.iter().flatten().filter(|p| **p != here).cloned().collect();
        tracelean_core::surface::welcome::welcome(
            vec![
                row("sandbox.new", "Start a sandbox for an agent (Claude Code, …)"),
                row("screen.station.requirements", "Requirements and their evidence"),
                row("screen.station.design", "The refinement graph"),
                row("trace.check", "Check the tree"),
                row("trace.findings", "Findings"),
                row("history.tree", "History"),
            ],
            &others,
        )
    }

    /// Keep the list of folders opened before (`recall::note_recent`), this
    /// one first, and show the others on the welcome page. A frontend asks
    /// for this; an editor opened by a suite keeps no such list.
    pub fn track_recent(&mut self) {
        self.recent = Some(recall::note_recent(&self.root));
        let fresh = self.welcome();
        if let Some(held) = self.screen.opened.iter_mut().find(|b| b.id == fresh.id) {
            *held = fresh;
        }
    }

    /// How wide the sandbox panel is: the pane showing it, else the side pane.
    fn side_width(&self) -> usize {
        let placed = self.laid_out(self.region);
        placed
            .iter()
            .find(|p| p.buffer.id == "record:sandbox")
            .or_else(|| placed.iter().find(|p| p.pane == tracelean_core::surface::screen::SIDE))
            .map(|p| (p.at.width as usize).saturating_sub(2))
            .filter(|w| *w > 0)
            .unwrap_or(60)
    }

    /// Look at the sandbox again, without anyone asking: what the agent has
    /// changed and said since the last look. The panel is re-produced in place
    /// when it is open, and nothing is focused or moved.
    ///
    /// Answers whether anything a person would see changed, so a frontend
    /// polling this redraws only when there is something new.
    pub fn tick(&mut self) -> bool {
        self.remember();
        if self.follow_disk() {
            return true;
        }
        // Counted here, between keys, rather than on one: it reads the tree.
        let mut recounted = self.recount_chips();
        if self.problems_stale.replace(false) {
            let count = Some(self.found().len());
            recounted |= self.problems.replace(count) != count;
        }
        let Some(live) = self.session.clone() else { return recounted };
        let agent = tracelean_core::observe::workspace::snapshot(std::path::Path::new(&live.work));
        let pending = tracelean_core::observe::mirror::mutations(self.workspace(), agent.clone());
        let moved = pending != self.pending;
        self.pending = pending;
        self.agent_tree = Some(agent);
        if moved {
            // The explorer marks what is waiting, and the status line says so
            // wherever the person is looking.
            self.refresh_views();
            // Files, as the panel lists them; a changed file is two commands.
            let files: BTreeSet<String> = self.pending.iter().map(touched).collect();
            match files.len() {
                0 => self.say("sandbox", "nothing is waiting"),
                1 => self.say("sandbox", "1 file changed by the agent is waiting — the sandbox panel lists it"),
                n => self.say("sandbox", &format!("{n} files changed by the agent are waiting — the sandbox panel lists them")),
            }
        }
        if !self.screen.opened.iter().any(|b| b.id == "record:sandbox") {
            return moved || recounted;
        }
        let fresh = self.sandbox();
        let held = self.screen.opened.iter_mut().find(|b| b.id == "record:sandbox");
        match held {
            Some(held) if *held != fresh => {
                *held = fresh;
                true
            }
            _ => moved || recounted,
        }
    }

    /// Take in what something else changed on disk — `git checkout`, another
    /// editor. A file not edited here is brought up to date, as a step the
    /// history can undo; a file edited here too keeps the edit, and the status
    /// line says that saving it overwrites the disk.
    ///
    /// Answers whether anything was taken in.
    pub fn follow_disk(&mut self) -> bool {
        let disk = tracelean_core::observe::workspace::snapshot(&self.root);
        if disk == self.saved {
            return false;
        }
        let now = self.workspace();
        let mut taken = Vec::new();
        let mut kept = BTreeSet::new();
        for command in tracelean_core::observe::mirror::mutations(self.saved.clone(), disk.clone()) {
            let path = touched(&command);
            if now.files.get(&path) == self.saved.files.get(&path) {
                taken.push(command);
            } else {
                kept.insert(path);
            }
        }
        let moved: BTreeSet<String> = taken.iter().map(touched).collect();
        if moved.is_empty() && kept.is_empty() {
            // Nothing a person edits differs; what does is not kept here.
            self.saved = disk;
            return false;
        }
        // One step, so one Ctrl+Z takes all of it back.
        if !taken.is_empty() {
            if let Err(refusal) = self.push(Command::Batch { commands: taken }) {
                self.say("could not follow the disk", &format!("{refusal:?}"));
                self.saved = disk;
                return true;
            }
        }
        for path in moved.iter().chain(&kept) {
            match disk.files.get(path) {
                Some(text) => self.saved.files.insert(path.clone(), text.clone()),
                None => self.saved.files.remove(path),
            };
        }
        self.forget_index();
        self.refresh_views();
        self.offset = self.offset.min(self.buffer().text.chars().count());
        let list = |paths: &BTreeSet<String>| paths.iter().cloned().collect::<Vec<_>>().join(", ");
        if !kept.is_empty() {
            self.say("changed on disk", &format!("{} — your edits are kept; saving overwrites the disk", list(&kept)));
        } else if !moved.is_empty() {
            self.say("changed on disk", &format!("{} reloaded (Ctrl+Z takes it back)", list(&moved)));
        }
        true
    }

    /// Make a sandbox and show it.
    fn create_session(&mut self) {
        match tracelean_core::observe::workcopy::create(&self.root, &self.workspace()) {
            Ok(made) => {
                let host = tracelean_core::observe::workcopy::host(&self.root);
                let _ = tracelean_core::observe::workcopy::write_launcher(&made, &host);
                self.session = Some(made);
                self.pending.clear();
                self.agent_tree = None;
                self.perform(Intent::Display { what: BufferKind::Record { title: "sandbox".into() } });
                self.say("sandbox ready", "copy the command, run it in your terminal, then start your agent");
            }
            Err(error) => self.say("could not make a sandbox", &error.to_string()),
        }
    }

    /// Put the command that enters the sandbox where the frontend can copy it.
    fn copy_command(&mut self) {
        let Some(live) = &self.session else {
            self.say("no sandbox", "make one first");
            return;
        };
        let host = tracelean_core::observe::workcopy::host(&self.root);
        if let Err(error) = tracelean_core::observe::workcopy::write_launcher(live, &host) {
            self.say("could not write the launcher", &error.to_string());
            return;
        }
        self.clipboard = Some(tracelean_core::observe::sandbox::short_command(live));
        self.say("copied", "paste it into your terminal, then start your agent inside it");
    }

    /// Discard the sandbox and whatever was not accepted from it.
    fn end_session(&mut self) {
        let Some(live) = self.session.take() else {
            self.say("no sandbox", "there is nothing to end");
            return;
        };
        self.pending.clear();
        self.agent_tree = None;
        match tracelean_core::observe::workcopy::destroy(&live) {
            Ok(()) => self.say("sandbox ended", "its copy is gone; what you accepted stays"),
            Err(error) => self.say("could not remove the sandbox", &error.to_string()),
        }
        self.reproduce_sandbox();
    }

    /// Re-produce the sandbox panel where it is, if it is open.
    fn reproduce_sandbox(&mut self) {
        if self.screen.opened.iter().any(|b| b.id == "record:sandbox") {
            let fresh = self.sandbox();
            if let Some(held) = self.screen.opened.iter_mut().find(|b| b.id == "record:sandbox") {
                *held = fresh;
            }
        }
    }

    fn menu_entries(&self, mode: &str) -> Vec<make::MenuEntry> {
        let Some((_, found)) = self.keymap.modes.iter().find(|(name, _)| name == mode) else {
            return Vec::new();
        };
        found
            .bindings
            .iter()
            .map(|(key, binding)| make::MenuEntry {
                key: key.clone(),
                description: binding.description().to_string(),
                action: match binding {
                    Binding::Dispatch { action, .. } => Some(action.clone()),
                    Binding::Enter { .. } => None,
                },
            })
            .collect()
    }

    /// What a report says.
    ///
    /// Every report here is a reading of this tree. The ones that would mean
    /// *running* something — differential testing, shrinking, judging — are the
    /// exception, and they answer with the command rather than with a result
    /// nobody produced: this editor starts no processes.
    ///
    /// @implements ARCH-NO-DRIVING.no_model_call
    fn report(&self, title: &str) -> Vec<Event> {
        match title {
            "check" | "findings" => self.findings(),
            "history" => self.history(),
            "observed" => self.observed(),
            "evidence" => self.evidence(),
            "lock" => self.lock(),
            "rollup" => self.rollup(),
            "stale" => self.stale(),
            "drt bindings" => self.bindings(),
            "drt run" => elsewhere("running the bindings", "cargo test -p tracelean-core -- --ignored"),
            "drt shrink" => elsewhere(
                "shrinking a divergence",
                "cargo test -p tracelean-core --test differential_shrink -- --ignored",
            ),
            "drt coverage" => {
                elsewhere("measuring binding coverage", "cargo test -p tracelean-core -- --ignored")
            }
            "drt judge" => vec![Event {
                kind: "yours".into(),
                text: "a judgement is a person's; record it in the lock file, not from here"
                    .into(),
            }],
            other => vec![Event {
                kind: "no such report".into(),
                text: format!("nothing produces a `{other}` report"),
            }],
        }
    }

    /// What each clause is claimed by, and what the lock says it is worth.
    ///
    /// The level comes from the committed lock, never from the tree: an
    /// annotation is a claim, and a claim is L1 until a backend earns more.
    fn evidence(&self) -> Vec<Event> {
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let records: Vec<tracelean_core::trace::record::Evidence> =
            lock.as_ref().map(|l| l.evidence.clone()).unwrap_or_default();
        let levels = levels_of(&records);

        let mut out = Vec::new();
        if lock.is_none() {
            out.push(Event {
                kind: "no lock".into(),
                text: "no lock file is committed, so no evidence is recorded and every clause \
                       reads L1"
                    .into(),
            });
        }
        for (id, requirement) in &index.requirements {
            for clause in requirement.clause_keys() {
                let mut roles: Vec<&str> = index
                    .links
                    .iter()
                    .filter(|link| &link.req_id == id && link.clause == clause)
                    .map(|link| link.role.as_str())
                    .collect();
                roles.sort();
                roles.dedup();
                let level = levels.get(&(id.clone(), clause.clone())).copied().unwrap_or(Level::L1);
                let claimed =
                    if roles.is_empty() { "nothing claims it".into() } else { roles.join(" ") };
                out.push(Event {
                    kind: format!("{level:?}").to_lowercase(),
                    // The chain beside the minimum, because the minimum is what
                    // a clause is worth and the chain is why. A proved model
                    // with an unchecked implementation reads L1, and without the
                    // chain the proof is invisible.
                    text: format!(
                        "{} {} — {claimed}",
                        named(id, &clause),
                        chain_of(&records, id, clause.as_deref())
                    ),
                });
            }
        }
        out
    }

    /// Render the index into the lock file and write it where the project keeps
    /// it, carrying the evidence already recorded through unchanged.
    ///
    /// @implements REQ-LOCK.evidence_preserved
    fn lock(&self) -> Vec<Event> {
        use tracelean_core::trace::lockfile;
        let index = tracelean_core::trace::index::build(&self.root);
        let held = lockfile::read(&self.root).map(|l| l.evidence).unwrap_or_default();
        let earned = tracelean_core::trace::store::read_all(&self.root);
        let collected = lockfile::collected(&index, held, earned);
        let rendered = &collected.lockfile;
        match lockfile::write(&self.root, rendered) {
            Err(error) => vec![Event { kind: "could not write".into(), text: error.to_string() }],
            Ok(path) => {
                let mut out = vec![
                    Event { kind: "written".into(), text: path.display().to_string() },
                    Event {
                        kind: "records".into(),
                        text: format!(
                            "{} requirements, {} links, {} evidence records",
                            rendered.requirements.len(),
                            rendered.links.len(),
                            rendered.evidence.len()
                        ),
                    },
                ];
                out.extend(collected.dropped.iter().map(|record| Event {
                    kind: "stale".into(),
                    text: format!("{} {:?}", named(&record.key.req_id, &record.key.clause), record.key.bond),
                }));
                out
            }
        }
    }

    /// Assurance and coverage over the refinement graph.
    ///
    /// One tree per requirement that refines nothing. Coverage of a requirement
    /// whose decomposition is open is written as a lower bound, because that is
    /// what it is.
    ///
    /// @implements REQ-ROLLUP.open_is_lower_bound
    fn rollup(&self) -> Vec<Event> {
        use tracelean_core::trace::rollup;
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let levels = levels_of(lock.as_ref().map(|l| &l.evidence[..]).unwrap_or(&[]));
        // The floor is the level at which a clause counts as covered: checked
        // by something that runs, rather than asserted or read.
        let floor = Level::L3;

        let roots: Vec<String> = index
            .requirements
            .iter()
            .filter(|(_, r)| r.refines.is_empty())
            .map(|(id, _)| id.clone())
            .collect();
        if roots.is_empty() {
            return vec![Event {
                kind: "nothing to roll up".into(),
                text: "no requirement stands at the top of the graph".into(),
            }];
        }
        let mut out = Vec::new();
        for root in roots {
            flatten(&rollup::tree(&index, &root, &levels, floor), 0, &mut out);
        }
        out
    }

    /// Which documents were written about what the tree still contains.
    ///
    /// *In review* is not an error — it is a document nobody has confirmed
    /// since the thing it describes moved.
    ///
    /// @implements REQ-DOCLINK.review_is_not_error
    fn stale(&self) -> Vec<Event> {
        use tracelean_core::trace::doclink::{self, State};
        let index = self.index();
        let current = doclink::pairs(&doclink::current_hashes(&index));
        if index.doc_links.is_empty() {
            return vec![Event {
                kind: "none".into(),
                text: "no document records what it describes".into(),
            }];
        }
        index
            .doc_links
            .iter()
            .map(|link| match doclink::state(link.clone(), current.clone()) {
                State::Current => Event {
                    kind: "current".into(),
                    text: format!("{} describes {}", link.file, link.target),
                },
                State::InReview { .. } => Event {
                    kind: "in review".into(),
                    text: format!("{} describes {}, which has changed since", link.file, link.target),
                },
                State::Dangling => Event {
                    kind: "dangling".into(),
                    text: format!("{} describes {}, which is gone", link.file, link.target),
                },
            })
            .collect()
    }

    /// Which clauses are bound to a model and an implementation, and to what.
    fn bindings(&self) -> Vec<Event> {
        match tracelean_core::drt::config::read(&self.root) {
            Err(error) => vec![Event { kind: "unreadable".into(), text: error }],
            Ok(bindings) if bindings.is_empty() => vec![Event {
                kind: "none".into(),
                text: "no clause is bound to a model and an implementation".into(),
            }],
            Ok(bindings) => bindings
                .iter()
                .map(|binding| {
                    let sides: Vec<String> = binding
                        .implementations()
                        .iter()
                        .map(|call| format!("{} {}", call.language, call.entry))
                        .collect();
                    Event {
                        kind: "binding".into(),
                        text: format!(
                            "{} establishes {} — {}",
                            binding.op(),
                            binding.clauses().len(),
                            sides.join(" | ")
                        ),
                    }
                })
                .collect(),
        }
    }

    fn findings(&self) -> Vec<Event> {
        let found = self.found();
        if found.is_empty() {
            return vec![Event { kind: "clean".into(), text: "nothing to report".into() }];
        }
        found
            .into_iter()
            .map(|f| Event { kind: f.kind, text: format!("{}:{} {}", f.file, f.line, f.message) })
            .collect()
    }

    /// What the checker finds in this tree.
    fn found(&self) -> Vec<tracelean_core::surface::findings_view::Found> {
        use tracelean_core::trace::checker::{check, Policy};
        let index = self.index();
        check(&index, &Policy::default())
            .iter()
            .map(|finding| tracelean_core::surface::findings_view::Found {
                kind: format!("{:?}", finding.kind).to_lowercase(),
                file: finding.file.clone(),
                line: finding.line,
                message: finding.message.clone(),
            })
            .collect()
    }

    /// Every node of the history, with what it was made after and what it did.
    fn points(&self) -> Vec<tracelean_core::surface::history_view::Point> {
        let here = self.tree.current();
        self.tree
            .ids()
            .iter()
            .map(|id| {
                let node = self.tree.node(*id).expect("an id the tree gave");
                tracelean_core::surface::history_view::Point {
                    node: id.0,
                    parent: node.parent.map(|p| p.0),
                    here: Some(*id) == here,
                    said: tracelean_core::history::command::describe(&node.command),
                }
            })
            .collect()
    }

    fn history(&self) -> Vec<Event> {
        if self.tree.ids().is_empty() {
            return vec![Event {
                kind: "base".into(),
                text: "nothing has been changed yet; this is the tree as it was opened".into(),
            }];
        }
        self.points()
            .into_iter()
            .map(|p| Event {
                kind: if p.here { "here".into() } else { "node".into() },
                text: format!("#{}  {}", p.node, p.said),
            })
            .collect()
    }

    /// What was seen in the working tree and not yet taken in.
    ///
    /// One line a change, naming the path, because the path is what a person
    /// decides about.
    fn observed(&self) -> Vec<Event> {
        if self.pending.is_empty() {
            return vec![Event {
                kind: "clean".into(),
                text: "the working tree matches the editor".into(),
            }];
        }
        self.pending
            .iter()
            .map(|command| Event {
                kind: match command {
                    Command::CreateFile { .. } => "created".into(),
                    Command::DeleteFile { .. } => "deleted".into(),
                    Command::RenameFile { .. } => "renamed".into(),
                    _ => "changed".into(),
                },
                text: touched(command),
            })
            .collect()
    }

    fn edit(&mut self, command: Command) {
        match self.push(command) {
            Ok(_) => {
                self.say("edited", "the change is in the history");
                self.refresh_views();
            }
            Err(refusal) => self.say("refused", &format!("{refusal:?}")),
        }
    }

    fn travel(&mut self, move_: Move) {
        // Whether the history moved is read off where it is, before and after.
        // `undo` answers with the node it arrived at, and undoing the very first
        // change arrives at no node at all — which is still a move, and was
        // reported as "nowhere to go" while the text quietly changed.
        let before = self.tree.current();
        self.anchor = None;
        // A run of typing is one step back and one forward, as in any editor;
        // the history still holds each key, which the history panel shows.
        let command = |tree: &Tree, id: Option<tracelean_core::history::tree::NodeId>| {
            id.and_then(|id| tree.node(id)).map(|n| n.command.clone())
        };
        match move_ {
            Move::Back => {
                let mut left = command(&self.tree, self.tree.current());
                self.tree.undo();
                while let (Some(gone), Some(here)) = (left.clone(), command(&self.tree, self.tree.current())) {
                    if !continues(&here, &gone) {
                        break;
                    }
                    left = Some(here);
                    self.tree.undo();
                }
                // The cursor goes to where the change was taken back.
                match left {
                    Some(Command::Insert { file, offset, .. }) if self.editing() == Some(file.clone()) => {
                        self.offset = offset;
                    }
                    Some(Command::Delete { file, offset, deleted }) if self.editing() == Some(file.clone()) => {
                        self.offset = offset + deleted.chars().count();
                    }
                    _ => {}
                }
            }
            Move::Forward => {
                let mut last = None;
                while let Some(at) = self.tree.redo() {
                    last = command(&self.tree, Some(at));
                    let next = self.tree.node(at).and_then(|n| n.children.iter().max().copied());
                    match (last.clone(), command(&self.tree, next)) {
                        (Some(here), Some(then)) if continues(&here, &then) => {}
                        _ => break,
                    }
                }
                match last {
                    Some(Command::Insert { file, offset, text }) if self.editing() == Some(file.clone()) => {
                        self.offset = offset + text.chars().count();
                    }
                    Some(Command::Delete { file, offset, .. }) if self.editing() == Some(file.clone()) => {
                        self.offset = offset;
                    }
                    _ => {}
                }
            }
            Move::Branch => {
                self.branch();
            }
            Move::To { node } => {
                let id = tracelean_core::history::tree::NodeId(node);
                if self.tree.node(id).is_some() {
                    if let Err(refusal) = self.tree.jump_to(id) {
                        self.say("refused", &format!("{refusal:?}"));
                        return;
                    }
                }
            }
        }
        let moved = self.tree.current() != before;
        if moved {
            self.say("travelled", "the workspace is at another point in the history");
            self.refresh_views();
        } else {
            self.say("nowhere to go", "there is no such point in the history");
        }
    }

    /// Move to the next sibling of where we are: another branch of the same
    /// parent, which is what a tree has and a stack does not.
    fn branch(&mut self) -> bool {
        let Some(here) = self.tree.current() else { return false };
        let Some(node) = self.tree.node(here) else { return false };
        let siblings = match node.parent {
            Some(parent) => self.tree.node(parent).map(|p| p.children.clone()).unwrap_or_default(),
            None => self.tree.roots(),
        };
        let Some(at) = siblings.iter().position(|id| *id == here) else { return false };
        let Some(next) = siblings.get((at + 1) % siblings.len().max(1)) else { return false };
        *next != here && self.tree.jump_to(*next).is_ok()
    }

    fn watch(&mut self, watch: Watch) {
        match watch {
            Watch::Start => self.observe(),
            Watch::Accept => self.accept(),
            Watch::Reject => self.reject(),
            Watch::Create => self.create_session(),
            Watch::Copy => self.copy_command(),
            Watch::Finish => self.end_session(),
            Watch::CopyAll => {
                self.clipboard = Some(self.buffer().text);
                self.say("copied", "the whole buffer is on the clipboard");
            }
            Watch::CopyLine => {
                let (line, _) = self.line_and_column();
                let text = tracelean_core::surface::view::plain_text(self.buffer())
                    .get(line)
                    .cloned()
                    .unwrap_or_default();
                // A report's row reads `kind: text`; what is worth copying is
                // the text — a command, a path.
                let text = match (self.buffer().kind, text.split_once(": ")) {
                    (BufferKind::Record { .. }, Some((_, rest))) => rest.to_string(),
                    _ => text,
                };
                self.say("copied", &text);
                self.clipboard = Some(text);
            }
            Watch::CopyPath { path } => {
                self.say("copied", &path);
                self.clipboard = Some(path);
            }
            Watch::AcceptFile { path } => self.accept_file(&path),
            Watch::RejectFile { path } => self.reject_file(&path),
            Watch::ContextToggle { part } => {
                if !self.context_parts.remove(&part) {
                    self.context_parts.insert(part);
                }
                // Drawn again where it is, the cursor on the switch it was on,
                // so several parts can be chosen in a row.
                if let BufferKind::Record { title } = self.buffer().kind {
                    let at = self.offset;
                    self.perform(Intent::Display { what: BufferKind::Record { title } });
                    self.offset = at.min(self.buffer().text.chars().count());
                }
            }
            Watch::ContextCopy => {
                use tracelean_core::surface::context::{context_text, gather};
                let target = match self.buffer().kind {
                    BufferKind::Record { title } => title.strip_prefix("context ").map(str::to_string),
                    _ => None,
                };
                let found = target.and_then(|t| gather(&t, &self.index(), &self.workspace().files));
                match found {
                    Some(context) => {
                        let text = context_text(&context, &self.context_parts);
                        self.say(
                            "copied",
                            &format!("{} characters of context for {} — paste them to your agent", text.chars().count(), context.target),
                        );
                        self.clipboard = Some(text);
                    }
                    None => self.say("nothing copied", "open the context of a requirement first (Space c o on its name)"),
                }
            }
        }
    }

    /// The trace of the file the document shows: each claim it makes, and
    /// everything else that claims the same clause.
    fn file_trace(&self) -> Buffer {
        use tracelean_core::surface::file_trace::{file_trace_view, Entry};
        use tracelean_core::surface::requirement_view::Claim;
        let width = self.pane_width(tracelean_core::surface::screen::SIDE);
        let Some((path, _)) = self.document_place() else {
            return file_trace_view("(no file open)", &[], width);
        };
        let index = self.index();
        let records: Vec<tracelean_core::trace::record::Evidence> = tracelean_core::trace::lockfile::read(&self.root)
            .map(|l| l.evidence)
            .unwrap_or_default();
        let levels = levels_of(&records);
        let claim_of = |link: &tracelean_core::trace::index::Link| Claim {
            role: link.role.as_str().to_string(),
            path: link.anchor.file.clone(),
            line: link.line,
            symbol: symbol_of(&link.anchor),
        };
        // A requirement's own document: each clause, and everything in the
        // tree that claims it — where the requirement is met, read from it.
        if let Some(requirement) = index.requirements.values().find(|r| r.file == path) {
            let text = self
                .screen
                .opened
                .iter()
                .find(|b| matches!(&b.kind, BufferKind::File { path: p } if *p == path))
                .map(|b| b.text.clone())
                .unwrap_or_default();
            let entries: Vec<Entry> = tracelean_core::surface::chips::clause_lines(&text)
                .into_iter()
                .filter(|(_, key)| requirement.clauses.contains_key(key))
                .map(|(line, key)| Entry {
                    line: line as u32 + 1,
                    symbol: Some(key.clone()),
                    role: "clause".into(),
                    clause: format!("{}.{key}", requirement.id),
                    text: requirement.clauses[&key].clone(),
                    level: levels.get(&(requirement.id.clone(), Some(key.clone()))).copied().unwrap_or(Level::L1),
                    others: index
                        .links
                        .iter()
                        .filter(|l| l.req_id == requirement.id && l.clause.as_deref() == Some(key.as_str()))
                        .map(claim_of)
                        .collect(),
                })
                .collect();
            return file_trace_view(&path, &entries, width);
        }
        let mut entries: Vec<Entry> = index
            .links
            .iter()
            .filter(|link| link.anchor.file == path)
            .map(|link| {
                let text = index
                    .requirements
                    .get(&link.req_id)
                    .map(|r| match &link.clause {
                        Some(k) => r.clauses.get(k).cloned().unwrap_or_default(),
                        None => r.title.clone(),
                    })
                    .unwrap_or_else(|| "no requirement is called this".into());
                let others = index
                    .links
                    .iter()
                    .filter(|o| o.req_id == link.req_id && o.clause == link.clause && o.anchor.ident() != link.anchor.ident())
                    .map(claim_of)
                    .collect();
                Entry {
                    line: link.line,
                    symbol: symbol_of(&link.anchor),
                    role: link.role.as_str().to_string(),
                    clause: match &link.clause {
                        Some(k) => format!("{}.{k}", link.req_id),
                        None => link.req_id.clone(),
                    },
                    text,
                    level: levels.get(&(link.req_id.clone(), link.clause.clone())).copied().unwrap_or(Level::L1),
                    others,
                }
            })
            .collect();
        entries.sort_by_key(|e| e.line);
        file_trace_view(&path, &entries, width)
    }

    /// What an agent needs to change a requirement or clause, with the parts
    /// the person has chosen marked.
    ///
    /// @implements REQ-CONTEXT.copied_not_sent
    fn agent_context(&self, target: &str) -> Buffer {
        use tracelean_core::surface::context::{context_view, gather};
        match gather(target, &self.index(), &self.workspace().files) {
            Some(context) => context_view(&context, &self.context_parts, self.pane_width(tracelean_core::surface::screen::DOCUMENT)),
            None => make::record_buffer(
                format!("context {target}"),
                vec![Event { kind: "unknown".into(), text: format!("no requirement is called {target}") }],
            ),
        }
    }

    /// Read the working tree and record how it differs from what the editor
    /// holds.
    ///
    /// This is the whole of "watching": no agent is started, no process is
    /// spawned, no model is called. Somebody else ran something, and this looks
    /// at what it left. The difference is derived by the mirror, so protected
    /// paths are excluded and the change list is the same one a differential
    /// test checks.
    ///
    /// @implements ARCH-NO-DRIVING.no_model_call
    /// @implements REQ-MIRROR.protected_excluded
    fn observe(&mut self) {
        // With a sandbox, the agent's copy is what is looked at, and the panel
        // that shows its changes is where they are listed.
        if self.session.is_some() {
            self.tick();
            let seen = self.pending.len();
            if seen == 0 {
                self.say("nothing yet", "the agent has not changed anything in the sandbox");
            } else {
                self.say("observed", &format!("{seen} changes in the sandbox; accept or reject them"));
            }
            self.perform(Intent::Display { what: BufferKind::Record { title: "sandbox".into() } });
            return;
        }
        let on_disk = tracelean_core::observe::workspace::snapshot(&self.root);
        self.pending = tracelean_core::observe::mirror::mutations(self.workspace(), on_disk.clone());
        self.agent_tree = Some(on_disk);
        if self.pending.is_empty() {
            self.say("nothing to observe", "the working tree matches the editor");
            return;
        }
        let seen = self.pending.len();
        self.say("observed", &format!("{seen} changes in the working tree; a accepts, x rejects"));
        self.perform(Intent::Display { what: BufferKind::Record { title: "observed".into() } });
    }

    /// Take the observed changes into the history.
    ///
    /// Through `tree.push`, like every other change, so that accepting an
    /// agent's work is undoable for the same reason typing is.
    ///
    /// @implements REQ-ACT.edits_are_commands
    fn accept(&mut self) {
        if self.pending.is_empty() {
            self.say("nothing to accept", "nothing has been observed");
            return;
        }
        let taken = std::mem::take(&mut self.pending);
        let count = taken.len();
        for command in taken {
            if let Err(refusal) = self.push(command) {
                self.say("refused", &format!("{refusal:?}"));
                return;
            }
        }
        if self.session.is_some() {
            // The agent wrote into a copy, so the real tree has not got the
            // change yet: accepting is what puts it there, deletions included.
            let workspace = self.workspace();
            if let Err(error) = tracelean_core::observe::workcopy::sync(&self.root, &workspace) {
                self.say("could not write the accepted changes", &error.to_string());
                return;
            }
        }
        self.saved = self.workspace();
        self.forget_index();
        self.say("accepted", &format!("{count} changes are in the history and the project; Ctrl+Z undoes them"));
        self.refresh_views();
        self.reproduce_sandbox();
    }

    /// Take in the waiting change to one file, and only that.
    ///
    /// @implements REQ-ACT.edits_are_commands
    fn accept_file(&mut self, path: &str) {
        let (taken, kept): (Vec<Command>, Vec<Command>) =
            std::mem::take(&mut self.pending).into_iter().partition(|c| touched(c) == path);
        self.pending = kept;
        if taken.is_empty() {
            self.say("nothing to accept", &format!("{path} has no change waiting"));
            return;
        }
        for command in taken {
            if let Err(refusal) = self.push(command) {
                self.say("refused", &format!("{refusal:?}"));
                return;
            }
        }
        if self.session.is_some() {
            if let Err(error) = self.write_one(&self.root.clone(), path) {
                self.say("could not write the accepted change", &error.to_string());
                return;
            }
            let workspace = self.workspace();
            if let Some(text) = workspace.files.get(path) {
                self.saved.files.insert(path.to_string(), text.clone());
            } else {
                self.saved.files.remove(path);
            }
        }
        self.forget_index();
        self.say("accepted", &format!("{path} is in the history and the project; Ctrl+Z undoes it"));
        self.refresh_views();
        self.reproduce_sandbox();
    }

    /// Take the waiting change to one file back out of the sandbox.
    fn reject_file(&mut self, path: &str) {
        let before = self.pending.len();
        self.pending.retain(|c| touched(c) != path);
        if self.pending.len() == before {
            self.say("nothing to reject", &format!("{path} has no change waiting"));
            return;
        }
        if let Some(live) = self.session.clone() {
            if let Err(error) = self.write_one(std::path::Path::new(&live.work), path) {
                self.say("could not reset the file in the sandbox", &error.to_string());
                return;
            }
            let _ = self.tick();
        }
        self.say("rejected", &format!("{path} was taken back out of the sandbox"));
        self.reproduce_sandbox();
    }

    /// Make one file under `dir` what the editor holds: written, or removed
    /// when the editor holds no such file.
    fn write_one(&self, dir: &Path, path: &str) -> std::io::Result<()> {
        let held = self.workspace().files.get(path).cloned();
        tracelean_core::observe::workcopy::put(dir, path, held.as_deref())
    }

    /// Drop the observed changes without taking them in.
    ///
    /// It does not touch the working tree. Rejecting means the editor will not
    /// carry the change, not that the file somebody else wrote is deleted —
    /// deleting another process's work on a keystroke is not a decision this
    /// editor makes for anybody.
    fn reject(&mut self) {
        if self.pending.is_empty() {
            self.say("nothing to reject", "nothing has been observed");
            return;
        }
        let count = self.pending.len();
        self.pending.clear();
        // In a sandbox the copy is the editor's to keep, so rejecting puts it
        // back to what the editor holds — otherwise the next look would find
        // the same changes again. The real tree was never touched.
        if let Some(live) = &self.session {
            let workspace = self.workspace();
            let work = std::path::PathBuf::from(&live.work);
            if let Err(error) = tracelean_core::observe::workcopy::sync(&work, &workspace) {
                self.say("could not reset the sandbox", &error.to_string());
                return;
            }
            self.agent_tree = Some(workspace);
            self.say("rejected", &format!("{count} changes were taken back out of the sandbox"));
            self.reproduce_sandbox();
            return;
        }
        self.say(
            "rejected",
            &format!("{count} changes were left out; the working tree still holds them"),
        );
    }

    /// Write the workspace out, and remember what was written.
    fn persist(&mut self) {
        let workspace = self.workspace();
        match tracelean_core::observe::workspace::write_into(&workspace, &self.root) {
            Ok(()) => {
                self.saved = workspace;
                self.forget_index();
                self.refresh_views();
                self.say("saved", "the working tree matches the editor");
            }
            Err(error) => self.say("could not save", &error.to_string()),
        }
    }

    fn refuse(&mut self, why: Blocked) {
        let text = match why {
            Blocked::UnknownAction { action } => format!("`{action}` is not an action"),
            Blocked::NeedsTarget { action, what } => format!("`{action}` needs {what}"),
        };
        self.say("nothing happened", &text);
    }
}

/// A report this editor will not produce, and the command that does.
///
/// Producing it would mean starting a process, and the one thing this editor
/// never does is drive something. Naming the command is more use than a blank
/// page and more honest than a result nobody computed.
///
/// @implements ARCH-HONEST.named_findings
fn elsewhere(what: &str, command: &str) -> Vec<Event> {
    vec![
        Event {
            kind: "not here".into(),
            text: format!("{what} runs processes; this editor starts none"),
        },
        Event { kind: "run".into(), text: command.to_string() },
    ]
}

/// The changed file a row of the sandbox panel lists: the text of the row's
/// span that reviews it.
fn changed_on_row(buffer: &Buffer, offset: usize) -> Option<String> {
    let chars: Vec<char> = buffer.text.chars().collect();
    let start = chars[..offset.min(chars.len())].iter().rposition(|c| *c == '\n').map_or(0, |n| n + 1);
    let end = chars[start..].iter().position(|c| *c == '\n').map_or(chars.len(), |n| start + n);
    buffer
        .spans
        .iter()
        .find(|s| s.start >= start && s.stop <= end && s.actions.iter().any(|a| a == "observe.diff"))
        .map(|s| chars[s.start..s.stop].iter().collect())
}

/// How a line is commented out in a file of this kind; None for text with no
/// line comment.
fn comment_marker(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("") {
        "lean" | "sql" | "hs" => Some("--"),
        "py" | "sh" | "toml" | "yaml" | "yml" | "rb" => Some("#"),
        "md" | "txt" | "json" => None,
        _ => Some("//"),
    }
}

/// Whether `next` carries on the typing `prev` did, so undo takes both at
/// once: a character typed right after the last one, up to the start of the
/// next word or line, or a Backspace right before the last one.
fn continues(prev: &Command, next: &Command) -> bool {
    match (prev, next) {
        (
            Command::Insert { file: a, offset: at, text: typed },
            Command::Insert { file: b, offset, text },
        ) => {
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { return false };
            let after_space = typed.ends_with(char::is_whitespace) && !c.is_whitespace();
            a == b && *offset == at + typed.chars().count() && c != '\n' && !typed.contains('\n') && !after_space
        }
        (
            Command::Delete { file: a, offset: at, .. },
            Command::Delete { file: b, offset, deleted },
        ) => a == b && deleted.chars().count() == 1 && offset + 1 == *at,
        _ => false,
    }
}

/// The path a command is about, for a list somebody reads before deciding.
fn touched(command: &Command) -> String {
    match command {
        Command::Insert { file, .. } | Command::Delete { file, .. } => file.clone(),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => path.clone(),
        Command::RenameFile { from, to } => format!("{from} → {to}"),
        Command::Batch { commands } => {
            commands.iter().map(touched).collect::<Vec<_>>().join(", ")
        }
    }
}

/// A clause, written the way a binding and a report both spell it.
fn named(req_id: &str, clause: &Option<String>) -> String {
    match clause {
        Some(clause) => format!("{req_id}.{clause}"),
        None => req_id.to_string(),
    }
}

/// `layout` with every split's weights restated as the cells each part takes
/// in `rect` — the same panes in the same places, in units of one cell.
fn in_cells(rect: Rect, layout: Layout) -> Layout {
    let Layout::Split { axis, parts } = layout else { return layout };
    let placed = place(rect, Layout::Split { axis, parts: parts.clone() });
    let parts = parts
        .into_iter()
        .map(|(weight, inner)| {
            let ids: Vec<String> = panes(&inner).into_iter().map(|(id, _)| id).collect();
            let rects: Vec<Rect> = placed.iter().filter(|(id, _)| ids.contains(id)).map(|(_, r)| *r).collect();
            let (Some(left), Some(top)) = (rects.iter().map(|r| r.left).min(), rects.iter().map(|r| r.top).min()) else {
                return (weight, inner);
            };
            let right = rects.iter().map(|r| r.left + r.width).max().unwrap_or(left);
            let bottom = rects.iter().map(|r| r.top + r.height).max().unwrap_or(top);
            let span = Rect { left, top, width: right - left, height: bottom - top };
            let size = match axis {
                tracelean_core::surface::screen::Axis::Across => span.width,
                tracelean_core::surface::screen::Axis::Down => span.height,
            };
            (size.max(1), in_cells(span, inner))
        })
        .collect();
    Layout::Split { axis, parts }
}

/// The item an annotation sits on, by name, when it sits on one.
fn symbol_of(anchor: &tracelean_core::trace::anchor::Anchor) -> Option<String> {
    match &anchor.kind {
        tracelean_core::trace::anchor::AnchorKind::Decl { symbol_path } => Some(symbol_path.clone()),
        _ => None,
    }
}

/// The level each clause reaches, from the evidence the lock file carries.
///
/// A clause's level is the minimum over its three bonds, and a record can never
/// count for more than its backend supports — so this goes through the same two
/// functions the rest of the project does rather than reading `level` directly.
///
/// @implements REQ-EVID.weakest_link
/// @implements REQ-EVID.judgement_caps
fn levels_of(
    evidence: &[tracelean_core::trace::record::Evidence],
) -> BTreeMap<(String, Option<String>), Level> {
    use tracelean_core::evidence::{assurance, Record};
    let mut collected: BTreeMap<(String, Option<String>), Vec<Record>> = BTreeMap::new();
    for record in evidence {
        collected
            .entry((record.key.req_id.clone(), record.key.clause.clone()))
            .or_default()
            .push(Record { bond: record.key.bond, level: record.effective_level() });
    }
    collected.into_iter().map(|(key, records)| (key, assurance(records))).collect()
}

/// The three bonds of one clause, written out.
///
/// `REQ-EVID.chain_rendered` exists because the single number a clause carries
/// is a minimum, and a minimum says nothing about which bond held it down. A
/// proved model whose implementation nothing compares reads `L1/L1/L4`, and
/// that is a different situation from `L1/L1/L1` calling for different work.
///
/// @implements REQ-EVID.chain_rendered
fn chain_of(
    records: &[tracelean_core::trace::record::Evidence],
    req_id: &str,
    clause: Option<&str>,
) -> String {
    use tracelean_core::evidence::{chain, Record};
    let here: Vec<Record> = records
        .iter()
        .filter(|record| record.key.req_id == req_id && record.key.clause.as_deref() == clause)
        .map(|record| Record { bond: record.key.bond, level: record.effective_level() })
        .collect();
    chain(here).iter().map(|level| format!("{level:?}")).collect::<Vec<_>>().join("/")
}

/// A roll-up tree, as lines. Depth is indentation, because a tree drawn as a
/// tree is a tree the frontend would have to know about.
fn flatten(node: &tracelean_core::trace::rollup::RollUp, depth: usize, out: &mut Vec<Event>) {
    out.push(Event {
        kind: format!("{:?}", node.assurance).to_lowercase(),
        text: format!(
            "{}{} covered {}",
            "  ".repeat(depth),
            node.id,
            node.covered.render()
        ),
    });
    for child in &node.children {
        flatten(child, depth + 1, out);
    }
}

/// Where the declarations are in a file, so that a reader can ask what evidence
/// each one carries.
///
/// A file the grammar cannot read is marked nowhere rather than marked wrongly.
/// The marks a file is shown with: its tokens, so it is syntax highlighted.
///
/// Declarations used to be marked whole, as headings — every function body one
/// purple block — and a buffer's spans cannot overlap, so a mark that size left
/// no room to say what was inside it.
fn marks_in(path: &str, text: &str) -> Vec<make::Mark> {
    let extension = Path::new(path).extension().and_then(|e| e.to_str());
    if extension == Some("md") {
        return tracelean_core::surface::highlight::markdown(text);
    }
    let lang = extension.and_then(Lang::from_extension);
    tracelean_core::surface::highlight::tokens(text, lang)
}
