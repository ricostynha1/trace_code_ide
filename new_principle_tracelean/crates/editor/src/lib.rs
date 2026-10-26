//! @implements REQ-SHOW.core_produces
//! @implements REQ-ACT.one_path

pub mod recall;
pub mod theme;

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use tracelean_core::evidence::Marks;
use tracelean_core::history::command::{Command, Workspace};
use tracelean_core::history::tree::Tree;
use tracelean_core::observe::transcript::Event;
use tracelean_core::surface::act::{dispatch, touched, Blocked, Focus, Intent, Move, Watch};
use tracelean_core::surface::keymap::{Binding, Keymap, Outcome};
use tracelean_core::surface::explorer;
use tracelean_core::surface::produce as make;
use tracelean_core::surface::requirement_view;
use tracelean_core::surface::screen::{
    arrange, close_buffer, destination, panes, place, workbench, Arrangement, Layout, Rect, Screen,
};
use tracelean_core::surface::view::{actions_at, Buffer, BufferKind};
use tracelean_core::trace::anchor::Lang;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Then {
    Find,
    FindLines,
    SetSetting,
    Choose,
}

#[derive(Debug, Clone)]
struct Asking {
    then: Then,
    action: String,
    target: Option<String>,
    question: String,
    pane: String,
    at: usize,
    answer: String,
}

pub struct Editor {
    pub root: PathBuf,
    pub keymap: Keymap,
    pub mode: String,
    /// @implements REQ-SCREEN.screen_is_a_value
    pub screen: Screen,
    pub region: Rect,
    pub offset: usize,
    pub anchor: Option<usize>,
    pub status: Buffer,
    /// @implements REQ-SHOW.menu_from_keymap
    pub menu: Option<Buffer>,
    pub top: usize,
    parked: BTreeMap<String, (usize, usize)>,
    places: BTreeMap<String, (usize, usize)>,
    back: Vec<(String, usize)>,
    forward: Vec<(String, usize)>,
    pub tree: Tree,
    held_state: std::cell::RefCell<Option<(Option<tracelean_core::history::tree::NodeId>, Workspace)>>,
    chips_held: std::cell::RefCell<BTreeMap<String, (String, Vec<tracelean_core::surface::chips::Chip>)>>,
    coverage_held: std::cell::RefCell<(Option<std::time::SystemTime>, tracelean_core::trace::lines::Coverage)>,
    saved: Workspace,
    /// @implements ARCH-NO-DRIVING.no_model_call
    pending: Vec<Command>,
    pub running: bool,
    pub session: Option<tracelean_core::observe::sandbox::Session>,
    agent_tree: Option<Workspace>,
    pub clipboard: Option<String>,
    pub open_folders: BTreeSet<String>,
    pub context_parts: BTreeSet<tracelean_core::surface::context::Part>,
    pub design_open: BTreeSet<String>,
    pub open_only: bool,
    typed: Vec<String>,
    pub history_filter: tracelean_core::surface::history_view::Filter,
    saved_at: BTreeSet<u64>,
    pub last_find: Option<String>,
    offering: Option<(String, usize, Vec<tracelean_core::surface::offer::Offer>)>,
    asking: Option<Asking>,
    lean: Option<tracelean_core::lean_server::LeanServer>,
    lean_project: Option<PathBuf>,
    lean_error: Option<String>,
    goal: String,
    goal_asked: Option<(String, String, usize)>,
    index_cache: std::cell::RefCell<Option<std::sync::Arc<tracelean_core::trace::index::Index>>>,
    problems: std::cell::Cell<Option<usize>>,
    problems_stale: std::cell::Cell<bool>,
    close_warned: bool,
    remembered: Option<recall::Recalled>,
    recent: Option<Vec<String>>,
}

const OFFER_KEYS: [&str; 35] = [
    "1", "2", "3", "4", "5", "6", "7", "8", "9", "a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l",
    "m", "n", "o", "p", "q", "r", "s", "t", "u", "v", "w", "x", "y", "z",
];

const SMALL_PROJECT: usize = 40;

use tracelean_core::surface::screen::{self, GOALS as PROOF};

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

#[derive(Debug, Clone)]
pub struct Placed {
    pub pane: String,
    pub at: Rect,
    pub buffer: Buffer,
    pub top: usize,
    pub focused: bool,
}

pub const INSERT: &str = "Insert";

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
    /// @implements REQ-LINECOV.uncovered_shown
    /// @implements REQ-LINECOV.stale_hidden
    pub fn coverage_shown(&self, buffer: &Buffer, top: usize, height: usize) -> Vec<(usize, u64, String)> {
        use tracelean_core::trace::lines::{current, markers};
        let BufferKind::File { path } = &buffer.kind else { return Vec::new() };
        let mut held = self.coverage_held.borrow_mut();
        if let Some(newer) = tracelean_core::drt::lines_run::read_if_newer(&self.root, held.0) {
            *held = (Some(newer.0), newer.1);
        }
        let hash = tracelean_core::trace::hash::text(&buffer.text);
        let Some(lines) = current(&held.1, path, &hash) else { return Vec::new() };
        markers(lines.clone(), top, height).into_iter().map(|m| (m.line, m.hits, m.said)).collect()
    }

    fn implementing_lines(
        &self,
        index: &tracelean_core::trace::index::Index,
        id: &str,
        clause: Option<&Option<String>>,
    ) -> Vec<tracelean_core::surface::coverage_view::Item> {
        use tracelean_core::trace::lines::current;
        let mut held = self.coverage_held.borrow_mut();
        if let Some(newer) = tracelean_core::drt::lines_run::read_if_newer(&self.root, held.0) {
            *held = (Some(newer.0), newer.1);
        }
        let files = &self.workspace().files;
        let mut items: Vec<tracelean_core::surface::coverage_view::Item> = Vec::new();
        for link in index.links.iter().filter(|l| {
            l.role == tracelean_core::trace::annotation::Role::Implements
                && l.req_id == id
                && clause.map_or(true, |c| l.clause == *c)
        }) {
            let Some(text) = files.get(&link.anchor.file) else { continue };
            let Some(lines) = current(&held.1, &link.anchor.file, &tracelean_core::trace::hash::text(text)) else { continue };
            let (start, end) = (link.anchor.start_line + 1, link.anchor.end_line + 1);
            if items.iter().any(|i| i.path == link.anchor.file && i.start == start) {
                continue;
            }
            items.push(tracelean_core::surface::coverage_view::Item {
                path: link.anchor.file.clone(),
                start,
                symbol: symbol_of(&link.anchor),
                clause: link.clause.clone(),
                lines: lines.iter().filter(|l| l.line >= start && l.line <= end).cloned().collect(),
                text: text.lines().map(str::to_string).skip(start as usize - 1).take((end - start + 1) as usize).collect(),
            });
        }
        items
    }

    /// @implements REQ-LINECOV.clause_summary
    fn clause_lines(&self, index: &tracelean_core::trace::index::Index, id: &str, clause: Option<&Option<String>>) -> Option<(u64, u64, u64)> {
        let items = self.implementing_lines(index, id, clause);
        (!items.is_empty()).then(|| tracelean_core::surface::coverage_view::total(&items))
    }

    pub fn chips_shown(&self, buffer: &Buffer, top: usize, height: usize) -> Vec<(usize, char, String)> {
        let BufferKind::File { path } = &buffer.kind else { return Vec::new() };
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

    pub fn open(root: PathBuf, keymap: Keymap) -> Editor {
        Editor::open_as(root, keymap, true)
    }

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
            coverage_held: std::cell::RefCell::new((None, Default::default())),
            saved: workspace,
            pending: Vec::new(),
            running: true,
            session: None,
            agent_tree: None,
            clipboard: None,
            open_folders: BTreeSet::new(),
            context_parts: tracelean_core::surface::context::default_parts(),
            design_open: BTreeSet::new(),
            open_only: false,
            history_filter: tracelean_core::surface::history_view::Filter::File,
            typed: Vec::new(),
            saved_at: BTreeSet::new(),
            last_find: None,
            offering: None,
            asking: None,
            lean: None,
            lean_project: None,
            lean_error: None,
            goal: String::new(),
            goal_asked: None,
            index_cache: std::cell::RefCell::new(None),
            problems: std::cell::Cell::new(None),
            problems_stale: std::cell::Cell::new(true),
            close_warned: false,
            remembered: None,
            recent: None,
        };
        let files: Vec<String> = editor.workspace().files.keys().cloned().collect();
        if files.len() <= SMALL_PROJECT {
            editor.open_folders = files.iter().flat_map(|f| explorer::holders(f)).collect();
        }
        editor.session = tracelean_core::observe::workcopy::list(&editor.root).into_iter().next();
        if let Some(live) = &editor.session {
            let host = tracelean_core::observe::workcopy::host(&editor.root);
            let _ = tracelean_core::observe::workcopy::write_launcher(live, &host);
        }
        let listing = editor.listing();
        let side = editor.produce(BufferKind::Menu { title: "design".into() });
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

    fn replace_buffer(&mut self, buffer: Buffer) {
        let current = self.buffer().id;
        match self.screen.opened.iter_mut().find(|held| held.id == current) {
            Some(held) => *held = buffer,
            None => self.screen.opened.push(buffer),
        }
    }

    pub fn waiting(&self) -> Vec<Command> {
        self.pending.clone()
    }

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

    fn push(
        &mut self,
        command: Command,
    ) -> Result<tracelean_core::history::tree::NodeId, tracelean_core::history::command::Refusal> {
        let state = self.workspace();
        let (id, after) = self.tree.push_from(&state, command)?;
        *self.held_state.borrow_mut() = Some((Some(id), after));
        Ok(id)
    }

    pub fn say(&mut self, kind: &str, text: &str) {
        self.status = make::record_buffer(
            "status".into(),
            vec![Event { kind: kind.to_string(), text: text.to_string() }],
        );
    }

    fn listing(&self) -> Buffer {
        self.listing_of(".")
    }

    fn tree_rows(&self) -> Vec<explorer::Row> {
        let files: Vec<String> = self.workspace().files.keys().cloned().collect();
        explorer::rows(&files, &self.open_folders)
    }

    fn is_tree(&self, buffer: &Buffer) -> bool {
        matches!(&buffer.kind, BufferKind::Directory { path } if *path == self.root.display().to_string())
    }

    fn tree_row_at(&self, buffer: &Buffer, offset: usize) -> Option<explorer::Row> {
        if !self.is_tree(buffer) {
            return None;
        }
        explorer::path_at(&self.tree_rows(), &buffer.text, offset).cloned()
    }

    fn toggle_folder(&mut self, path: &str) {
        if !self.open_folders.remove(path) {
            self.open_folders.insert(path.to_string());
        }
        self.refresh_views();
    }

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

    fn is_folder(&self, path: &str) -> bool {
        let workspace = self.workspace();
        let inside = format!("{}/", path.trim_end_matches('/'));
        !workspace.files.contains_key(path) && workspace.files.keys().any(|f| f.starts_with(&inside))
    }

    /// @implements REQ-SHOW.listing_from_entries
    fn listing_of(&self, path: &str) -> Buffer {
        let under = path.trim_end_matches('/');
        if under == "." {
            let mut changed: BTreeMap<String, String> = BTreeMap::new();
            for command in &self.pending {
                let kind = match command {
                    Command::CreateFile { .. } => "created",
                    Command::DeleteFile { .. } => "deleted",
                    _ => "modified",
                };
                changed.entry(touched(command)).or_insert_with(|| kind.to_string());
            }
            for path in self.unsaved() {
                changed.entry(path).or_insert_with(|| "unsaved".to_string());
            }
            let current = self
                .laid_out(self.region)
                .into_iter()
                .find(|p| p.pane == tracelean_core::surface::screen::DOCUMENT)
                .and_then(|p| match p.buffer.kind {
                    BufferKind::File { path } => Some(path),
                    _ => None,
                });
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

    pub fn focus(&self) -> Focus {
        let buffer = self.buffer();
        let mut focus = focus_in(&buffer, self.offset);
        if buffer.id == "menu:opened" {
            focus.under = self.opened_at_row(&buffer, self.offset).map(|held| held.id);
        }
        if buffer.id == "record:sandbox" {
            let here = actions_at(buffer.clone(), self.offset);
            if here.iter().any(|a| a == "observe.accept_file" || a == "observe.reject_file") {
                focus.under = changed_on_row(&buffer, self.offset);
            }
        }
        if self.is_tree(&buffer) {
            focus.under = self.tree_row_at(&buffer, self.offset).map(|row| row.path);
        }
        if matches!(&buffer.kind, BufferKind::Menu { title } if title == "design") {
            let carried = actions_at(buffer.clone(), self.offset)
                .iter()
                .find_map(|a| a.strip_prefix("design.toggle ").map(str::to_string));
            if carried.is_some() {
                focus.under = carried;
            }
        }
        let requirement = matches!(&buffer.kind, BufferKind::Record { title } if title.starts_with("requirement "));
        if requirement && actions_at(buffer.clone(), self.offset).iter().any(|a| a == "trace.approve") {
            focus.under = tracelean_core::surface::view::plain_text(buffer).get(2).cloned();
        }
        focus
    }

    fn opened_at_row(&self, strip: &Buffer, offset: usize) -> Option<Buffer> {
        let row = strip.text.chars().take(offset).filter(|c| *c == '\n').count();
        self.screen.opened.get(row).cloned()
    }

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
        let intent = dispatch(action.to_string(), focus, self.workspace(), self.pending.clone());
        self.perform(intent);
    }

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

    pub fn offered(&self) -> Vec<String> {
        actions_at(self.buffer(), self.offset)
    }

    /// @implements REQ-SCREEN.buffer_goes_home
    fn show(&mut self, buffer: Buffer) {
        let home = destination(buffer.kind.clone(), self.screen.clone());
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

    /// @implements REQ-SCREEN.strip_is_the_opened_set
    pub fn strip(&self) -> Buffer {
        tracelean_core::surface::screen::strip_with_unsaved(self.screen.clone(), &self.unsaved())
    }

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

    pub fn remember(&mut self) {
        let Some(kept) = &self.remembered else { return };
        let now = self.recall();
        if *kept != now {
            recall::save(&self.root, &now);
            self.remembered = Some(now);
        }
    }

    /// @implements REQ-NAV.session_recalled
    fn restore(&mut self, kept: recall::Recalled) {
        let files = self.workspace().files;
        self.open_folders = kept.folders.into_iter().filter(|f| self.is_folder(f)).collect();
        let existing: Vec<String> = files.keys().cloned().collect();
        for path in tracelean_core::surface::reopen::reopened(kept.files, kept.shown, existing) {
            self.perform(Intent::Display { what: BufferKind::File { path } });
        }
    }

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

    /// @implements REQ-SCREEN.stations_are_constant
    pub fn stations(&self) -> Buffer {
        tracelean_core::surface::screen::stations(self.screen.clone())
    }

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

    pub fn grab(&mut self, pane: &str, cells: i64) {
        self.perform(Intent::Arrange { how: Arrangement::FocusPane { pane: pane.to_string() } });
        if cells == 0 {
            return;
        }
        if self.region.width > 0 && self.region.height > 0 {
            self.screen.layout = in_cells(self.region, self.screen.layout.clone());
        }
        self.perform(Intent::Arrange { how: Arrangement::Resize { amount: cells } });
    }

    /// @implements REQ-SHOW.window_is_a_buffer
    pub fn visible(&self, height: usize) -> Buffer {
        make::window(self.buffer(), self.top, height)
    }

    pub fn cursor_in(&self, height: usize) -> Option<(usize, usize)> {
        let (line, column) = self.line_and_column();
        if line < self.top || line >= self.top + height {
            return None;
        }
        Some((line - self.top, column))
    }

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

    pub fn step(&mut self, forwards: bool) {
        let length = self.buffer().text.chars().count();
        self.offset = if forwards {
            (self.offset + 1).min(length)
        } else {
            self.offset.saturating_sub(1)
        };
    }

    fn step_lines(&mut self, delta: i64) {
        let lines: Vec<String> = tracelean_core::surface::view::plain_text(self.buffer());
        let (line, column) = self.line_and_column();
        let last = lines.len().saturating_sub(1) as i64;
        let target = (line as i64 + delta).clamp(0, last) as usize;
        let width = lines.get(target).map_or(0, |l| l.chars().count());
        let at: usize = lines.iter().take(target).map(|before| before.chars().count() + 1).sum();
        self.offset = at + column.min(width);
    }

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

    fn editing(&self) -> Option<String> {
        match self.buffer().kind {
            BufferKind::File { path } => Some(path),
            _ => None,
        }
    }

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

    /// @implements REQ-ACT.edits_are_commands
    pub fn delete_back(&mut self) {
        let Some(file) = self.editing() else {
            self.say("not a file", "this buffer is a view of something else");
            return;
        };
        if self.offset == 0 {
            return;
        }
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

    fn change(&mut self, command: Command) {
        let Some(path) = self.editing() else { return };
        let before = self.workspace().files.get(&path).cloned().unwrap_or_default();
        let command = in_bytes(&before, command);
        let was_saved = self.saved.files.get(&path).map(String::as_str) == Some(self.buffer().text.as_str());
        match self.push(command) {
            Ok(_) => {
                let text = self.workspace().files.get(&path).cloned().unwrap_or_default();
                let marks = marks_in(&path, &text);
                let saved = self.saved.files.get(&path) == Some(&text);
                self.replace_buffer(make::file_buffer(path, text, marks));
                if saved != was_saved {
                    self.refresh_views();
                }
            }
            Err(refusal) => self.say("refused", &format!("{refusal:?}")),
        }
    }

    fn chars_before(&self, file: &str, offset: usize) -> usize {
        let text = self.workspace().files.get(file).cloned().unwrap_or_default();
        text.get(..offset).map_or_else(|| text.chars().count(), |head| head.chars().count())
    }

    fn refresh_views(&mut self) {
        let workspace = self.workspace();
        let gone: Vec<String> = self
            .screen
            .opened
            .iter()
            .filter(|held| matches!(&held.kind, BufferKind::File { path } if !workspace.files.contains_key(path)))
            .map(|held| held.id.clone())
            .collect();
        for id in gone {
            self.screen = close_buffer(id, self.screen.clone());
        }
        let fresh: Vec<Buffer> = self
            .screen
            .opened
            .iter()
            .map(|held| match &held.kind {
                BufferKind::Directory { path } if *path == self.root.display().to_string() => {
                    self.listing_of(".")
                }
                BufferKind::Directory { path } => self.listing_of(path),
                BufferKind::Record { title } if title == "history" => self.history_buffer(),
                BufferKind::File { path } => match workspace.files.get(path) {
                    Some(text) => make::file_buffer(path.clone(), text.clone(), marks_in(path, text)),
                    None => held.clone(),
                },
                _ => held.clone(),
            })
            .collect();
        for (held, new) in self.screen.opened.iter_mut().zip(fresh) {
            if held.id == new.id {
                *held = new;
            }
        }
        let length = self.buffer().text.chars().count();
        self.offset = self.offset.min(length);
    }

    fn buffer_in(&self, pane: &str) -> Option<Buffer> {
        let id = panes(&self.screen.layout).into_iter().find(|(p, _)| p == pane)?.1;
        self.screen.opened.iter().find(|held| held.id == id).cloned()
    }

    fn top_of(&self, pane: &str) -> usize {
        if pane == self.screen.focus {
            self.top
        } else {
            self.parked.get(pane).map(|(_, top)| *top).unwrap_or(0)
        }
    }

    fn absolute(&self, pane: &str, at: usize) -> usize {
        let Some(buffer) = self.buffer_in(pane) else { return at };
        let top = self.top_of(pane);
        let lines = tracelean_core::surface::view::plain_text(buffer.clone());
        let shift: usize = lines.iter().take(top).map(|line| line.chars().count() + 1).sum();
        (shift + at).min(buffer.text.chars().count())
    }

    pub fn focus_pane(&mut self, pane: &str) {
        if pane != self.screen.focus {
            self.rearrange(Arrangement::FocusPane { pane: pane.to_string() });
        }
    }

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

    /// @implements REQ-ACT.everything_is_offered
    pub fn offers_at(&self, pane: &str, at: usize) -> Vec<tracelean_core::surface::offer::Offer> {
        let Some(buffer) = self.buffer_in(pane) else { return Vec::new() };
        let mut offset = self.absolute(pane, at);
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
                        keys: None,
                    });
                }
            }
            offered.splice(at..at, extra);
        }
        offered
    }

    pub fn offers_here(&self) -> (String, usize, Vec<tracelean_core::surface::offer::Offer>) {
        let pane = self.screen.focus.clone();
        let at = self.offset.saturating_sub(self.absolute(&pane, 0));
        let offered = self.offers_at(&pane, at);
        (pane, at, offered)
    }

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
                if let Some(chosen) = answer.or(target) {
                    focus.under = Some(chosen);
                }
            }
        }
        let intent = dispatch(action.to_string(), focus, self.workspace(), self.pending.clone());
        let approving = action == "trace.approve" && matches!(intent, Intent::Edit { .. });
        self.perform(intent);
        if approving {
            self.say("approved", "the requirement is approved in the editor; Ctrl+S writes it");
        }
        self.mode = self.keymap.root.clone();
        self.menu = None;
    }

    pub fn chord(&mut self, chord: &str) {
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
            "F3" | "C-g" => return self.find_again(true),
            "S-F3" | "C-S-g" => return self.find_again(false),
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
            let intent = dispatch(action.to_string(), self.focus(), self.workspace(), self.pending.clone());
            self.perform(intent);
            return;
        }
        let page = (self.region.height as usize).saturating_sub(1).max(1);
        match chord {
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

    pub fn find(&mut self, wanted: &str, forward: bool) {
        if wanted.is_empty() {
            return;
        }
        self.last_find = Some(wanted.to_string());
        let spans: Vec<(usize, usize)> = if wanted.contains(['*', '?']) {
            tracelean_core::surface::find::glob_matches(self.buffer().text, wanted.to_string())
        } else {
            let text: Vec<char> = self.buffer().text.chars().collect();
            let needle: Vec<char> = wanted.chars().collect();
            (0..text.len().saturating_sub(needle.len() - 1))
                .filter(|at| text[*at..].starts_with(&needle))
                .map(|at| (at, at + needle.len()))
                .collect()
        };
        let hits: Vec<usize> = spans.iter().map(|(start, _)| *start).collect();
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
                self.anchor = Some(at);
                self.offset = spans.iter().find(|(start, _)| *start == at).map_or(at, |(_, end)| *end);
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

    /// @implements REQ-ACT.edits_are_commands
    pub fn delete_forward(&mut self) {
        let Some(file) = self.editing() else { return };
        let at = self.offset;
        let Some(deleted) = self.buffer().text.chars().nth(at) else { return };
        self.change(Command::Delete { file, offset: at, deleted: deleted.to_string() });
        self.offset = at;
    }

    pub fn scroll(&mut self, pane: &str, lines: i64) {
        self.focus_pane(pane);
        let count = self.buffer().text.split('\n').count();
        self.step_lines(lines);
        let top = self.top as i64 + lines;
        self.top = top.clamp(0, count.saturating_sub(1) as i64) as usize;
    }

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

    pub fn act_here(&mut self, action: &str) {
        self.offering = None;
        let intent = dispatch(action.to_string(), self.focus(), self.workspace(), self.pending.clone());
        self.perform(intent);
        self.mode = self.keymap.root.clone();
        if self.offering.is_none() {
            self.menu = None;
        }
    }

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

    fn start_asking(&mut self, action: &str) -> bool {
        if !matches!(action, "file.rename" | "file.new" | "trace.new_requirement") {
            return false;
        }
        let pane = self.screen.focus.clone();
        let at = self.window_offset(&pane, self.offset);
        let Some(offer) = self.offers_at(&pane, at).into_iter().find(|o| o.action == action && o.asks.is_some())
        else {
            return false;
        };
        let question = offer.asks.clone().unwrap_or_default();
        let answer = offer.target.clone().unwrap_or_default();
        self.asking = Some(Asking { then: Then::Choose, action: offer.action, target: offer.target, question, pane, at, answer });
        self.show_question();
        true
    }

    fn show_question(&mut self) {
        if let Some(asking) = &self.asking {
            let text = format!("{}: {}▏", asking.question, asking.answer);
            self.say("ask", &text);
        }
    }

    fn ask_to_find(&mut self, title: &str) {
        let (then, question) = if title == "find" {
            (Then::Find, "Find (* and ? match)")
        } else {
            (Then::FindLines, "List the lines matching (* and ? match)")
        };
        let pane = self.screen.focus.clone();
        let at = self.window_offset(&pane, self.offset);
        let answer = self.last_find.clone().unwrap_or_default();
        self.asking = Some(Asking { then, action: String::new(), target: None, question: question.into(), pane, at, answer });
        self.show_question();
    }

    /// @implements REQ-NAV.find_lists_lines
    pub fn list_matching_lines(&mut self, wanted: &str) {
        self.last_find = Some(wanted.to_string());
        let lines = tracelean_core::surface::find::matching_lines(self.buffer().text, wanted.to_string());
        let count = lines.len();
        let events: Vec<Event> = lines
            .into_iter()
            .map(|(number, line)| Event { kind: format!("line {number}"), text: line })
            .collect();
        self.show(make::record_buffer(format!("lines matching {wanted}"), events));
        self.say("found", &format!("{count} lines match {wanted}"));
    }

    fn answer(&mut self, key: &str) {
        let Some(mut asking) = self.asking.take() else { return };
        let (outcome, said) =
            tracelean_core::surface::ask::ask_step(asking.answer.clone(), key.to_string());
        match outcome.as_str() {
            "cancelled" => self.say("left", "nothing was changed"),
            "done" => match asking.then {
                Then::Find => self.find(&said, true),
                Then::FindLines => self.list_matching_lines(&said),
                Then::SetSetting => {
                    if let Some(name) = asking.target {
                        self.set_setting(&name, &said);
                    }
                }
                Then::Choose => self.choose(&asking.pane, asking.at, &asking.action, asking.target, Some(said)),
            },
            _ => {
                asking.answer = said;
                self.asking = Some(asking);
                self.show_question();
            }
        }
    }

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

    fn window_offset(&self, pane: &str, offset: usize) -> usize {
        let Some(buffer) = self.buffer_in(pane) else { return offset };
        let top = self.top_of(pane);
        let lines = tracelean_core::surface::view::plain_text(buffer);
        let shift: usize = lines.iter().take(top).map(|l| l.chars().count() + 1).sum();
        offset.saturating_sub(shift)
    }

    pub fn list_occurrences(&mut self, needle: &str, whole: bool) {
        use tracelean_core::surface::findings_view::{findings_view, Found};
        let workspace = self.workspace();
        let found = tracelean_core::surface::definition::occurrences(workspace.files.into_iter().collect(), needle, whole, 500);
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

    fn go_to_definition(&mut self, name: String) {
        use tracelean_core::surface::definition::declarations;
        let buffer = self.buffer();
        let named = buffer
            .spans
            .iter()
            .find(|s| s.role == tracelean_core::surface::view::Role::Requirement && s.start <= self.offset && self.offset < s.stop)
            .map(|s| buffer.text.chars().skip(s.start).take(s.stop - s.start).collect::<String>());
        if let Some(name) = named {
            let (id, clause) = { let (r, c) = tracelean_core::drt::config::split_op(&name); (r.to_string(), c.map(String::from)) };
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
        let workspace = self.workspace();
        let found = declarations(workspace.files.into_iter().collect(), &name);
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

    fn index(&self) -> std::sync::Arc<tracelean_core::trace::index::Index> {
        if let Some(held) = self.index_cache.borrow().as_ref() {
            return held.clone();
        }
        let fresh = std::sync::Arc::new(tracelean_core::trace::index::build(&self.root));
        *self.index_cache.borrow_mut() = Some(fresh.clone());
        fresh
    }

    fn forget_index(&self) {
        *self.index_cache.borrow_mut() = None;
        self.problems_stale.set(true);
    }

    pub fn problems(&self) -> Option<usize> {
        self.problems.get()
    }

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

    pub fn requirement_text(&self, name: &str) -> Option<String> {
        let (id, clause) = tracelean_core::drt::config::split_op(&name);
        let index = self.index();
        let requirement = index.requirements.get(id)?;
        Some(match clause.and_then(|c| requirement.clauses.get(c)) {
            Some(said) => format!("{name}: {said}"),
            None => format!("{id}: {}", requirement.title),
        })
    }

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
        tracelean_core::surface::quick::matches(files, query, 12).into_iter().map(at).collect()
    }

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

    pub fn over_lines(&mut self, pane: &str, start: usize, end: usize, chord: &str) {
        let (from, to) = {
            let (a, b) = (self.absolute(pane, start), self.absolute(pane, end));
            (a.min(b), a.max(b))
        };
        self.focus_pane(pane);
        self.over_range(from, to, chord);
    }

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
        let first = lines.first().map(|(at, _)| *at).unwrap_or(0);
        let spanned = match lines.last() {
            Some((last, _)) => text.chars().skip(first).take(last - first).filter(|c| *c == '\n').count(),
            None => 0,
        };
        self.change(Command::Batch { commands });
        self.offset = self.offset.min(self.buffer().text.chars().count());
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

    pub fn paste(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.take_selection();
        self.insert(text);
    }

    pub fn selection(&self) -> Option<(usize, usize)> {
        let anchor = self.anchor?;
        let length = self.buffer().text.chars().count();
        let (a, b) = (anchor.min(length), self.offset.min(length));
        (a != b).then(|| (a.min(b), a.max(b)))
    }

    pub fn selected(&self) -> Option<String> {
        let (from, to) = self.selection()?;
        Some(self.buffer().text.chars().skip(from).take(to - from).collect())
    }

    pub fn selection_shown(&self) -> Option<(usize, usize)> {
        let (from, to) = self.selection()?;
        let pane = self.screen.focus.clone();
        let shift = self.offset - self.window_offset(&pane, self.offset);
        (to > shift).then(|| (from.saturating_sub(shift), to - shift))
    }

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

    pub fn select(&mut self, pane: &str, start: usize, end: usize) {
        self.place(pane, end);
        self.anchor = Some(self.absolute(pane, start));
    }

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

    fn mode_menu(&mut self, mode: String) -> Buffer {
        let menu = self.produce(BufferKind::Menu { title: mode });
        let head = self.typed.join(" ") + "  …";
        let shift = head.chars().count() + 1;
        let mut spans = vec![tracelean_core::surface::view::Span {
            start: 0,
            stop: head.chars().count(),
            role: tracelean_core::surface::view::Role::Heading,
            actions: Vec::new(),
        }];
        spans.extend(menu.spans.iter().map(|s| tracelean_core::surface::view::Span { start: s.start + shift, stop: s.stop + shift, ..s.clone() }));
        Buffer { text: format!("{head}\n{}", menu.text), spans, ..menu }
    }

    /// @implements REQ-ACT.one_path
    pub fn key(&mut self, key: &str) {
        if self.mode == self.keymap.root {
            self.typed.clear();
        }
        if self.offering.is_some() {
            self.pick_offer(key);
            return;
        }
        if self.asking.is_some() {
            self.answer(key);
            return;
        }
        match tracelean_core::surface::keymap::step(
            self.keymap.clone(),
            self.mode.clone(),
            key.to_string(),
        ) {
            Outcome::Enter { mode } if mode == INSERT => {
                if self.editing().is_none() {
                    self.say("not a file", "there is nothing here to type into");
                } else {
                    self.mode = mode;
                    self.menu = None;
                    self.say("insert", "Escape leaves");
                }
            }
            Outcome::Enter { mode } if mode == self.keymap.root => {
                self.mode = mode;
                self.menu = None;
                self.typed.clear();
                self.say("normal", ". lists what can be done here, Space opens the leader menu");
            }
            Outcome::Enter { mode } => {
                self.mode = mode.clone();
                self.typed.push(key.to_string());
                self.menu = Some(self.mode_menu(mode));
            }
            Outcome::Leave { mode } => {
                self.mode = mode.clone();
                self.typed.pop();
                self.menu = if mode == self.keymap.root {
                    self.typed.clear();
                    None
                } else {
                    Some(self.mode_menu(mode))
                };
                self.say("left", "back one mode");
            }
            Outcome::Dispatch { action } if self.start_asking(&action) => {
                self.mode = self.keymap.root.clone();
                self.typed.clear();
                self.menu = None;
            }
            Outcome::Dispatch { action } => {
                let intent = dispatch(action, self.focus(), self.workspace(), self.pending.clone());
                self.perform(intent);
                self.mode = self.keymap.root.clone();
                self.typed.clear();
                if self.offering.is_none() {
                    self.menu = None;
                }
            }
            Outcome::PassThrough => self.typed(key),
        }
    }

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

    fn duplicate_line(&mut self) {
        let Some((file, start, line)) = self.this_line() else { return };
        let end = start + line.chars().count();
        let length = line.chars().count();
        self.change(Command::Insert { file, offset: end, text: format!("\n{line}") });
        self.offset += length + 1;
    }

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

    fn indent_step(&self) -> usize {
        match self.editing() {
            Some(path) if path.ends_with(".lean") => 2,
            _ => 4,
        }
    }

    fn before_cursor(&self) -> String {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let at = self.offset.min(text.len());
        let start = text[..at].iter().rposition(|c| *c == '\n').map_or(0, |i| i + 1);
        text[start..at].iter().collect()
    }

    fn indent_here(&self) -> String {
        self.before_cursor().chars().take_while(|c| *c == ' ' || *c == '\t').collect()
    }

    fn inside_pair(&self) -> bool {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let (before, after) = (self.offset.checked_sub(1).and_then(|i| text.get(i)), text.get(self.offset));
        matches!((before, after), (Some('('), Some(')')) | (Some('['), Some(']')) | (Some('{'), Some('}')))
    }

    fn inside_quotes(&self) -> bool {
        let text: Vec<char> = self.buffer().text.chars().collect();
        let before = self.offset.checked_sub(1).and_then(|i| text.get(i));
        before == Some(&'"') && text.get(self.offset) == Some(&'"')
    }

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

    fn line_break(&self) -> String {
        let before = self.before_cursor();
        let indent = self.indent_here();
        let end = before.trim_end();
        let opens = matches!(end.chars().last(), Some('{' | '(' | '[' | ':'))
            || [":=", " by", " where", " do"].iter().any(|word| end.ends_with(word));
        let deeper = if opens { " ".repeat(self.indent_step()) } else { String::new() };
        format!("\n{indent}{deeper}")
    }

    fn typed(&mut self, key: &str) {
        if self.mode != INSERT {
            self.anchor = None;
            match key {
                "Left" | "Right" if self.tree_step(key == "Right") => {}
                "Left" => self.step(false),
                "Right" => self.step(true),
                "Up" => self.step_line(false),
                "Down" => self.step_line(true),
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
        if let Some((from, to)) = self.selection() {
            let text: String = self.buffer().text.chars().skip(from).take(to - from).collect();
            match key {
                "Tab" if text.contains('\n') => return self.over_range(from, to, "Tab"),
                "Backspace" => {
                    self.take_selection();
                    return;
                }
                "Left" | "Right" | "Up" | "Down" => self.anchor = None,
                _ if key == "Enter" || key == "Tab" || key == tracelean_core::surface::keymap::SPACE || key.chars().count() == 1 => {
                    self.take_selection();
                }
                _ => {}
            }
        }
        match key {
            "Enter" => {
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
            tracelean_core::surface::keymap::SPACE => self.type_char(" "),
            other if other.chars().count() == 1 => self.type_char(other),
            other => self.say("nothing happened", &format!("`{other}` is not text")),
        }
    }

    /// @implements REQ-ACT.action_to_intent
    pub fn perform(&mut self, intent: Intent) {
        match intent {
            Intent::Display { what: BufferKind::File { path } }
                if Path::new(&path).is_absolute() && Path::new(&path).is_dir() =>
            {
                self.reopen(PathBuf::from(path));
            }
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
            Intent::Display { what: BufferKind::Record { title } } if title.starts_with("edit ") => {
                self.ask_to_edit_setting(&title["edit ".len()..]);
            }
            Intent::Display { what: BufferKind::Record { title } } if title == "find" || title == "find lines" => {
                self.ask_to_find(&title);
            }
            Intent::Display { what: BufferKind::Record { title } }
                if title.starts_with("definition ") =>
            {
                self.go_to_definition(title["definition ".len()..].to_string());
            }
            Intent::Display { what: BufferKind::Record { title } }
                if title.starts_with("references ") =>
            {
                self.list_occurrences(&title["references ".len()..], true);
            }
            Intent::Display { what: BufferKind::Menu { title } } if title == "offers" => {
                self.offer_here();
            }
            Intent::Display { what: BufferKind::File { path } } if self.is_folder(&path) => {
                self.toggle_folder(&path);
            }
            Intent::Display { what } => {
                if let BufferKind::File { path } = &what {
                    if self.workspace().files.contains_key(path) {
                        self.open_folders.extend(explorer::holders(path));
                    }
                }
                let lean = matches!(&what, BufferKind::File { path } if path.ends_with(".lean"));
                let buffer = self.produce(what);
                self.show(buffer);
                if lean {
                    self.goals_beside();
                }
                if self.screen.opened.iter().any(|b| self.is_tree(b)) {
                    self.refresh_views();
                }
            }
            Intent::Edit { command } => {
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

    fn goals_beside(&mut self) {
        let opened: Vec<String> = self.screen.opened.iter().map(|held| held.id.clone()).collect();
        let document = self.screen.focus.clone();
        let Some(BufferKind::File { path }) = self.screen.opened.iter().find(|held| held.id == self.buffer().id).map(|held| held.kind.clone()) else { return };
        if !screen::opens_goals(BufferKind::File { path }, opened) {
            return;
        }
        if destination(BufferKind::Record { title: "proof".into() }, self.screen.clone()) == document {
            let before = panes(&self.screen.layout);
            self.rearrange(Arrangement::Split { axis: screen::Axis::Across });
            if let Some((fresh, _)) = panes(&self.screen.layout).into_iter().find(|p| !before.contains(p)) {
                self.rearrange(Arrangement::FocusPane { pane: fresh });
            }
        }
        let proof = self.proof_buffer();
        self.show(proof);
        self.rearrange(Arrangement::FocusPane { pane: document });
    }

    fn rearrange(&mut self, how: Arrangement) {
        let was = self.screen.focus.clone();
        let before: Vec<String> = self.screen.opened.iter().map(|held| held.id.clone()).collect();
        self.screen = arrange(how, self.region, self.screen.clone());
        let after: Vec<String> = self.screen.opened.iter().map(|held| held.id.clone()).collect();
        for id in screen::closed_with(before, after) {
            self.screen = close_buffer(id, self.screen.clone());
        }
        if self.screen.focus != was {
            self.parked.insert(was, (self.offset, self.top));
            let (offset, top) = self.parked.remove(&self.screen.focus).unwrap_or((0, 0));
            self.offset = offset;
            self.top = top;
        }
        let length = self.buffer().text.chars().count();
        self.offset = self.offset.min(length);
        let placed: Vec<String> =
            panes(&self.screen.layout).into_iter().map(|(pane, _)| pane).collect();
        self.parked.retain(|pane, _| placed.contains(pane));
    }

    /// @implements REQ-CONTEXT.views_from_the_shell
    pub fn view(&mut self, what: BufferKind) -> Buffer {
        self.produce(what)
    }

    fn produce(&mut self, what: BufferKind) -> Buffer {
        match what {
            BufferKind::File { path } => {
                let text = self.workspace().files.get(&path).cloned().unwrap_or_default();
                let marks = marks_in(&path, &text);
                make::file_buffer(path, text, marks)
            }
            BufferKind::Directory { path } => self.listing_of(&path),
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
            BufferKind::Menu { title } if title == "opened" => self.strip(),
            BufferKind::Menu { title } if title == "requirements" || title == "design" => {
                let nodes = self.nodes();
                if nodes.is_empty() {
                    return make::menu_buffer(
                        title,
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
                match title.as_str() {
                    "design" => make::design_buffer(nodes, self.design_open.iter().cloned().collect(), self.open_only),
                    _ => make::requirements_buffer(nodes, self.open_only),
                }
            }
            BufferKind::Menu { title } if title == "welcome" => self.welcome(),
            BufferKind::Menu { title } => {
                let entries = self.menu_entries(&title);
                make::menu_buffer(title, entries)
            }
            BufferKind::Record { title } if title == "sandbox" => self.sandbox(),
            BufferKind::Record { title } if title == "history" => self.history_buffer(),
            BufferKind::Record { title } if title == "findings" || title == "check" => {
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
            BufferKind::Record { title } if title.starts_with("coverage ") => {
                self.coverage(&title["coverage ".len()..])
            }
            BufferKind::Record { title } if title == "keys" => make::record_buffer(title, self.keys()),
            BufferKind::Record { title } if title == "definitions" => self.definitions_buffer(),
            BufferKind::Record { title } if title == "proof" => self.proof_buffer(),
            BufferKind::Record { title } => {
                let events = self.report(&title);
                make::record_buffer(title, events)
            }
        }
    }

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
        let height = (self.region.height as usize).saturating_sub(1).max(1);
        if wanted < self.top || wanted >= self.top + height {
            self.top = wanted.saturating_sub(3);
        }
    }

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
        let held = marks_of(&index, &records);
        let plans = tracelean_core::trace::pinning::plans(&index, &self.workspace().files);
        let clauses = requirement
            .clause_keys()
            .into_iter()
            .map(|clause| {
                let pins = plans.iter().find(|p| p.req_id == id && p.clause == clause).map(|plan| {
                    use tracelean_core::{drt::pins::read_record, trace::pinning::shown};
                    shown(plan, read_record(&self.root, id, clause.as_deref()))
                });
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
                    narrowings: clause
                        .as_ref()
                        .and_then(|k| requirement.narrowings.get(k))
                        .map(|n| n.iter().map(|(k, v)| (k.clone(), v.clone())).collect())
                        .unwrap_or_default(),
                    marks: held.get(&(id.to_string(), clause.clone())).copied().unwrap_or(Marks::NONE),
                    tested: held.get(&(id.to_string(), clause.clone())).is_some_and(|m| m.tested),
                    lines: self.clause_lines(&index, id, Some(&clause)),
                    judged: tracelean_core::surface::requirement_view::judged_of(&records, id, clause.as_deref()),
                    key: clause,
                    claims,
                    pins,
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
            body: if requirement.clauses.is_empty() { String::new() } else { requirement.body.clone() },
            clauses,
            width: self.pane_width(tracelean_core::surface::screen::DOCUMENT),
            lines: self.clause_lines(&index, id, None),
        })
    }

    /// @implements REQ-LINECOV.lines_listed
    fn coverage(&self, named: &str) -> Buffer {
        let (id, clause) = { let (r, c) = tracelean_core::drt::config::split_op(&named); (r, c.map(String::from)) };
        let index = self.index();
        let scope = if clause.is_some() || index.requirements.get(id).is_some_and(|r| r.clauses.is_empty()) {
            Some(clause.clone())
        } else {
            None
        };
        let items = self.implementing_lines(&index, id, scope.as_ref());
        tracelean_core::surface::coverage_view::coverage_view(named, &items, self.pane_width(tracelean_core::surface::screen::DOCUMENT))
    }

    /// @implements REQ-JUDGE.prompt_exported
    fn judge(&self, name: &str) -> Buffer {
        use tracelean_core::trace::material;
        let (req_id, clause) = tracelean_core::drt::config::split_op(&name);
        let index = self.index();
        let refuse = |why: String| {
            make::record_buffer(format!("judge {name}"), vec![Event { kind: "cannot judge".into(), text: why }])
        };
        let Some(model_at) = material::model_of(&index, req_id, clause) else {
            return refuse(format!("nothing models {name}, so there is nothing to judge it against"));
        };
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

    fn pane_width(&self, pane: &str) -> usize {
        self.laid_out(self.region)
            .iter()
            .find(|p| p.pane == pane)
            .map(|p| (p.at.width as usize).saturating_sub(2))
            .filter(|w| *w > 0)
            .unwrap_or_else(|| self.side_width())
    }

    fn nodes(&self) -> Vec<make::Node> {
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let held = marks_of(&index, lock.as_ref().map(|l| &l.evidence[..]).unwrap_or(&[]));
        index
            .requirements
            .iter()
            .map(|(id, requirement)| {
                let clauses = requirement.clause_keys();
                let each: Vec<(Marks, bool)> = clauses
                    .iter()
                    .map(|clause| {
                        let implemented = index.links.iter().any(|link| {
                            &link.req_id == id
                                && &link.clause == clause
                                && link.role == tracelean_core::trace::annotation::Role::Implements
                        });
                        (held.get(&(id.clone(), clause.clone())).copied().unwrap_or(Marks::NONE), implemented)
                    })
                    .collect();
                make::node_of(
                    id.clone(),
                    requirement.title.clone(),
                    requirement.refines.clone(),
                    each,
                    self.clause_lines(&index, id, None).map(|(run, all, _)| (run, all)),
                )
            })
            .collect()
    }

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
                row("screen.station.design", "Requirements and their evidence, as the refinement graph"),
                row("trace.check", "Check the tree"),
                row("trace.findings", "Findings"),
                row("history.tree", "History"),
            ],
            &others,
        )
    }

    pub fn track_recent(&mut self) {
        self.recent = Some(recall::note_recent(&self.root));
        let fresh = self.welcome();
        if let Some(held) = self.screen.opened.iter_mut().find(|b| b.id == fresh.id) {
            *held = fresh;
        }
    }

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

    fn follow_proof(&mut self) -> bool {
        if !self.screen.opened.iter().any(|held| held.id == PROOF) {
            return false;
        }
        let mut changed = false;
        if let Some(said) = self.lean.as_mut().and_then(|server| server.poll()) {
            self.goal = said;
            changed = true;
        }
        let Some((path, offset)) = self.document_place().filter(|(path, _)| path.ends_with(".lean")) else {
            return self.refresh_proof(changed);
        };
        let text = self.workspace().files.get(&path).cloned().unwrap_or_default();
        let asked = (path.clone(), text.clone(), offset);
        if self.goal_asked.as_ref() == Some(&asked) {
            return self.refresh_proof(changed);
        }
        let file = self.root.join(&path);
        let Some(project) = tracelean_core::lean_server::project_of(&file, &self.root) else {
            self.lean_error = Some(format!("{path} is in no Lean project (no lean-toolchain or lakefile above it)"));
            self.goal_asked = Some(asked);
            return self.refresh_proof(true);
        };
        if self.lean_project.as_ref() != Some(&project) {
            self.lean = None;
            self.lean_error = None;
            match tracelean_core::lean_server::LeanServer::start(
                &project,
                std::env::var_os("ELAN_HOME")
                    .map(PathBuf::from)
                    .or_else(|| std::env::var_os("HOME").map(|home| PathBuf::from(home).join(".elan")))
                    .map(|dir| dir.join("bin").join("lake")),
            ) {
                Ok(server) => self.lean = Some(server),
                Err(why) => self.lean_error = Some(format!("{why} (reopen the editor to try again)")),
            }
            self.lean_project = Some(project);
        }
        if self.lean.is_none() {
            self.goal_asked = Some(asked);
            return self.refresh_proof(true);
        }
        let before: String = text.chars().take(offset).collect();
        let line = before.matches('\n').count();
        let column = before.rsplit('\n').next().map_or(0, |last| last.chars().count());
        if let Some(server) = self.lean.as_mut() {
            self.lean_error = server.ask(&file, &text, line, column).err();
        }
        if self.lean_error.is_some() {
            self.lean = None;
        }
        self.goal_asked = Some(asked);
        self.refresh_proof(changed || self.lean_error.is_some())
    }

    fn refresh_proof(&mut self, changed: bool) -> bool {
        if !changed {
            return false;
        }
        let fresh = self.proof_buffer();
        if let Some(held) = self.screen.opened.iter_mut().find(|held| held.id == PROOF) {
            *held = fresh;
        }
        true
    }

    fn proof_buffer(&self) -> Buffer {
        let text = match (&self.lean_error, self.goal.is_empty()) {
            (Some(why), _) => why.clone(),
            (None, true) => "Open a Lean file; the goals at the cursor are shown here.".to_string(),
            (None, false) => self.goal.clone(),
        };
        Buffer {
            id: PROOF.into(),
            kind: BufferKind::Record { title: "proof".into() },
            text,
            spans: Vec::new(),
        }
    }

    pub fn tick(&mut self) -> bool {
        self.remember();
        if self.follow_disk() {
            return true;
        }
        let mut recounted = self.recount_chips();
        recounted |= self.follow_proof();
        if self.problems_stale.replace(false) {
            let count = Some(self.found().len());
            recounted |= self.problems.replace(count) != count;
        }
        let Some(live) = self.session.clone() else { return recounted };
        let agent = tracelean_core::observe::workspace::snapshot(std::path::Path::new(&live.work));
        let base = tracelean_core::observe::workcopy::base(&live);
        let pending = tracelean_core::observe::mirror::changed_since(base, self.workspace(), agent.clone());
        let moved = pending != self.pending;
        self.pending = pending;
        self.agent_tree = Some(agent);
        if moved {
            self.refresh_views();
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
            self.saved = disk;
            return false;
        }
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

    fn definitions_buffer(&self) -> Buffer {
        use tracelean_core::surface::view::{Role, Span};
        let theme = theme::load(&self.root);
        let mut lines = vec![
            format!("Definitions — Space f e (or a click) changes the setting under the cursor; it is saved to {} at once", theme::OVERRIDE),
            "keys       assets/keymap.json: every binding is data; invalid ones are reported at load".to_string(),
            "languages  assets/languages/: the grammar and the declaring words of each language".to_string(),
            String::new(),
        ];
        let mut spans = Vec::new();
        let mut at: usize = lines.iter().map(|line| line.chars().count() + 1).sum();
        if let serde_json::Value::Object(sections) = &theme {
            for (section, entries) in sections {
                let serde_json::Value::Object(entries) = entries else { continue };
                for (key, value) in entries {
                    let Some(colour) = value.as_str() else { continue };
                    let name = format!("{section}.{key}");
                    let line = format!("{name}  {colour}");
                    spans.push(Span {
                        start: at,
                        stop: at + name.chars().count(),
                        role: Role::Path,
                        actions: vec![format!("definitions.edit {name}")],
                    });
                    at += line.chars().count() + 1;
                    lines.push(line);
                }
            }
        }
        Buffer {
            id: "record:definitions".into(),
            kind: BufferKind::Record { title: "definitions".into() },
            text: lines.join("\n"),
            spans,
        }
    }

    fn ask_to_edit_setting(&mut self, name: &str) {
        let current = theme::load(&self.root)
            .get(name.split('.').next().unwrap_or_default())
            .and_then(|section| section.get(name.split_once('.').map_or("", |(_, key)| key)))
            .and_then(|value| value.as_str().map(str::to_string))
            .unwrap_or_default();
        let pane = self.screen.focus.clone();
        let at = self.window_offset(&pane, self.offset);
        self.asking = Some(Asking {
            then: Then::SetSetting,
            action: String::new(),
            target: Some(name.to_string()),
            question: format!("New value for {name} (#rrggbb)"),
            pane,
            at,
            answer: current,
        });
        self.show_question();
    }

    pub fn set_setting(&mut self, name: &str, value: &str) {
        let Some((section, key)) = name.split_once('.') else {
            return self.say("not a setting", name);
        };
        if !tracelean_core::surface::setting::is_colour(value.to_string()) {
            return self.say("refused", &format!("`{value}` is not a colour like #1e90ff; nothing changed"));
        }
        let written = theme::write_setting(&self.root, section, key, value);
        match written {
            Ok(()) => {
                self.show(self.definitions_buffer());
                self.say("setting saved", &format!("{name} = {value}"));
            }
            Err(error) => self.say("could not save", &error.to_string()),
        }
    }

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

    fn evidence(&self) -> Vec<Event> {
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let records: Vec<tracelean_core::trace::record::Evidence> =
            lock.as_ref().map(|l| l.evidence.clone()).unwrap_or_default();
        let held = marks_of(&index, &records);

        let mut out = Vec::new();
        if lock.is_none() {
            out.push(Event {
                kind: "no lock".into(),
                text: "no lock file is committed, so no evidence is recorded and no clause \
                       holds more than its annotation"
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
                let marks = held.get(&(id.clone(), clause.clone())).copied().unwrap_or(Marks::NONE);
                let claimed =
                    if roles.is_empty() { "nothing claims it".into() } else { roles.join(" ") };
                out.push(Event { kind: marks.text(), text: format!("{} — {claimed}", named(id, &clause)) });
            }
        }
        out
    }

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

    /// @implements REQ-ROLLUP.open_is_lower_bound
    fn rollup(&self) -> Vec<Event> {
        use tracelean_core::trace::rollup;
        let index = self.index();
        let lock = tracelean_core::trace::lockfile::read(&self.root);
        let held = marks_of(&index, lock.as_ref().map(|l| &l.evidence[..]).unwrap_or(&[]));
        let floor = Marks { tested: true, ..Marks::NONE };

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
            flatten(&rollup::tree(&index, &root, &held, floor), 0, &mut out);
        }
        out
    }

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
                    file: file_of(&node.command),
                    saved: self.saved_at.contains(&id.0),
                }
            })
            .collect()
    }

    fn history_buffer(&self) -> Buffer {
        let file = self.document_place().map(|(path, _)| path);
        tracelean_core::surface::history_view::history_view(&self.points(), self.history_filter, file.as_deref())
    }

    pub fn history_preview_at(&self, pane: &str, at: usize) -> Option<Buffer> {
        let buffer = self.buffer_in(pane)?;
        let offset = self.absolute(pane, at);
        let span = buffer.spans.iter().find(|s| s.start <= offset && offset < s.stop && s.actions.iter().any(|a| a == "history.jump"))?;
        let name: String = buffer.text.chars().skip(span.start).take(span.stop - span.start).collect();
        self.history_preview(tracelean_core::surface::history_view::named_point(&name)??)
    }

    /// @implements REQ-UNDO.hover_shows_change
    pub fn history_preview(&self, node: u64) -> Option<Buffer> {
        let id = tracelean_core::history::tree::NodeId(node);
        let (before, after) = self.tree.preview(id).ok()?;
        let said = tracelean_core::history::command::describe(&self.tree.node(id)?.command);
        let paths: BTreeSet<&String> = before.files.keys().chain(after.files.keys()).collect();
        let changed: Vec<(String, String, String)> = paths
            .into_iter()
            .filter(|p| before.files.get(*p) != after.files.get(*p))
            .map(|p| {
                let text = |w: &Workspace| w.files.get(p).cloned().unwrap_or_default();
                (p.clone(), text(&before), text(&after))
            })
            .collect();
        Some(tracelean_core::surface::history_view::change_view(node, &said, &changed))
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
                text: format!("{}  {}", tracelean_core::surface::history_view::point_name(Some(p.node)), p.said),
            })
            .collect()
    }

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
        let before = self.tree.current();
        self.anchor = None;
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
                match left {
                    Some(Command::Insert { file, offset, .. }) if self.editing() == Some(file.clone()) => {
                        self.offset = self.chars_before(&file, offset);
                    }
                    Some(Command::Delete { file, offset, deleted }) if self.editing() == Some(file.clone()) => {
                        self.offset = self.chars_before(&file, offset) + deleted.chars().count();
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
                        self.offset = self.chars_before(&file, offset) + text.chars().count();
                    }
                    Some(Command::Delete { file, offset, .. }) if self.editing() == Some(file.clone()) => {
                        self.offset = self.chars_before(&file, offset);
                    }
                    _ => {}
                }
            }
            Move::Branch => {
                self.branch();
            }
            Move::Base => while self.tree.undo().is_some() {},
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
            Watch::Accept { command } => self.accept(command),
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
            Watch::AcceptFile { path, command } => self.accept_file(&path, command),
            Watch::RejectFile { path } => self.reject_file(&path),
            Watch::ContextToggle { part } => {
                if !self.context_parts.remove(&part) {
                    self.context_parts.insert(part);
                }
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
            Watch::HistoryFilter { filter } => {
                self.history_filter = filter;
                self.refresh_views();
            }
            Watch::DesignToggle { id } => {
                if !self.design_open.remove(&id) {
                    self.design_open.insert(id);
                }
                self.redraw_design();
            }
            Watch::DesignExpand { id } => {
                let nodes = self.nodes();
                self.design_open.extend(make::design_unfolded(&nodes, id.as_deref()));
                self.redraw_design();
            }
            Watch::DesignFold => {
                self.design_open.clear();
                self.redraw_design();
            }
            Watch::DesignOpenOnly => {
                self.open_only = !self.open_only;
                let index = BufferKind::Menu { title: "requirements".into() };
                match self.buffer().kind == index {
                    true => self.perform(Intent::Display { what: index }),
                    false => self.redraw_design(),
                }
            }
        }
    }

    fn redraw_design(&mut self) {
        let design = BufferKind::Menu { title: "design".into() };
        let here = self.buffer().kind == design;
        let at = self.offset;
        self.perform(Intent::Display { what: design });
        if here {
            self.offset = at.min(self.buffer().text.chars().count());
        }
    }

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
        let held = marks_of(&index, &records);
        let marks = |id: &str, clause: Option<String>| held.get(&(id.to_string(), clause)).copied().unwrap_or(Marks::NONE);
        let claim_of = |link: &tracelean_core::trace::index::Link| Claim {
            role: link.role.as_str().to_string(),
            path: link.anchor.file.clone(),
            line: link.line,
            symbol: symbol_of(&link.anchor),
        };
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
                    marks: marks(&requirement.id, Some(key.clone())),
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
                    marks: marks(&link.req_id, link.clause.clone()),
                    others,
                }
            })
            .collect();
        entries.sort_by_key(|e| e.line);
        file_trace_view(&path, &entries, width)
    }

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

    /// @implements ARCH-NO-DRIVING.no_model_call
    /// @implements REQ-MIRROR.protected_excluded
    fn observe(&mut self) {
        if self.session.is_some() {
            self.tick();
            let seen = self.pending.len();
            let known = self.session.as_ref().and_then(tracelean_core::observe::workcopy::base).is_some();
            if !known {
                self.say("unknown start", "this sandbox predates what TraceLean keeps of a copy's start, so its changes cannot be told from the project's; open a new sandbox");
            } else if seen == 0 {
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

    /// @implements REQ-ACT.edits_are_commands
    fn accept(&mut self, command: Command) {
        let taken = match command {
            Command::Batch { commands } => commands,
            other => vec![other],
        };
        if taken.is_empty() {
            self.say("nothing to accept", "nothing has been observed");
            return;
        }
        self.pending.clear();
        let count = taken.len();
        for command in taken {
            if let Err(refusal) = self.push(command) {
                self.say("refused", &format!("{refusal:?}"));
                return;
            }
        }
        if self.session.is_some() {
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

    /// @implements REQ-ACT.edits_are_commands
    fn accept_file(&mut self, path: &str, command: Command) {
        let taken = match command {
            Command::Batch { commands } => commands,
            other => vec![other],
        };
        self.pending.retain(|c| touched(c) != path);
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

    fn write_one(&self, dir: &Path, path: &str) -> std::io::Result<()> {
        let held = self.workspace().files.get(path).cloned();
        tracelean_core::observe::workcopy::put(dir, path, held.as_deref())
    }

    fn reject(&mut self) {
        if self.pending.is_empty() {
            self.say("nothing to reject", "nothing has been observed");
            return;
        }
        let count = self.pending.len();
        self.pending.clear();
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

    fn persist(&mut self) {
        let workspace = self.workspace();
        match tracelean_core::observe::workspace::write_into(&workspace, &self.root) {
            Ok(()) => {
                self.saved = workspace;
                if let Some(here) = self.tree.current() {
                    self.saved_at.insert(here.0);
                }
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

fn comment_marker(path: &str) -> Option<&'static str> {
    match Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("") {
        "lean" | "sql" | "hs" => Some("--"),
        "py" | "sh" | "toml" | "yaml" | "yml" | "rb" => Some("#"),
        "md" | "txt" | "json" => None,
        _ => Some("//"),
    }
}

fn in_bytes(text: &str, command: Command) -> Command {
    let bytes = |chars: usize| text.char_indices().nth(chars).map_or(text.len(), |(at, _)| at);
    match command {
        Command::Insert { file, offset, text: typed } => Command::Insert { file, offset: bytes(offset), text: typed },
        Command::Delete { file, offset, deleted } => Command::Delete { file, offset: bytes(offset), deleted },
        Command::Batch { commands } => {
            Command::Batch { commands: commands.into_iter().map(|c| in_bytes(text, c)).collect() }
        }
        other => other,
    }
}

fn continues(prev: &Command, next: &Command) -> bool {
    match (prev, next) {
        (
            Command::Insert { file: a, offset: at, text: typed },
            Command::Insert { file: b, offset, text },
        ) => {
            let mut chars = text.chars();
            let (Some(c), None) = (chars.next(), chars.next()) else { return false };
            let after_space = typed.ends_with(char::is_whitespace) && !c.is_whitespace();
            a == b && *offset == at + typed.len() && c != '\n' && !typed.contains('\n') && !after_space
        }
        (
            Command::Delete { file: a, offset: at, .. },
            Command::Delete { file: b, offset, deleted },
        ) => a == b && deleted.chars().count() == 1 && offset + deleted.len() == *at,
        _ => false,
    }
}

fn named(req_id: &str, clause: &Option<String>) -> String {
    tracelean_core::drt::config::qualified_op(req_id, clause.as_deref())
}

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

fn file_of(command: &Command) -> Option<String> {
    match command {
        Command::Insert { file, .. } | Command::Delete { file, .. } => Some(file.clone()),
        Command::CreateFile { path } | Command::DeleteFile { path, .. } => Some(path.clone()),
        Command::RenameFile { to, .. } => Some(to.clone()),
        Command::Batch { commands } => {
            let files: BTreeSet<Option<String>> = commands.iter().map(file_of).collect();
            match files.into_iter().collect::<Vec<_>>().as_slice() {
                [one] => one.clone(),
                _ => None,
            }
        }
    }
}

fn symbol_of(anchor: &tracelean_core::trace::anchor::Anchor) -> Option<String> {
    match &anchor.kind {
        tracelean_core::trace::anchor::AnchorKind::Decl { symbol_path } => Some(symbol_path.clone()),
        _ => None,
    }
}

/// @implements REQ-EVID.mark_per_bond
/// @implements REQ-EVID.judgement_caps
/// @implements REQ-EVID.structural_not_applicable
fn marks_of(
    index: &tracelean_core::trace::index::Index,
    evidence: &[tracelean_core::trace::record::Evidence],
) -> BTreeMap<(String, Option<String>), Marks> {
    use tracelean_core::evidence::{marks, Record};
    let mut collected: BTreeMap<(String, Option<String>), Vec<Record>> = BTreeMap::new();
    for record in evidence {
        collected
            .entry((record.key.req_id.clone(), record.key.clause.clone()))
            .or_default()
            .push(Record { bond: record.key.bond, level: record.effective_level() });
    }
    let mut out = BTreeMap::new();
    for (id, requirement) in &index.requirements {
        for clause in requirement.clause_keys() {
            let key = (id.clone(), clause);
            let claims: Vec<_> =
                index.links.iter().filter(|link| link.req_id == key.0 && link.clause == key.1).collect();
            let records = collected.remove(&key).unwrap_or_default();
            let held = marks(records, !claims.is_empty());
            let by_reading = claims.iter().any(|link| link.is_structural())
                && !claims.iter().any(|link| link.role == tracelean_core::trace::annotation::Role::Models);
            out.insert(key, if by_reading { tracelean_core::evidence::structural(held) } else { held });
        }
    }
    out
}

fn flatten(node: &tracelean_core::trace::rollup::RollUp, depth: usize, out: &mut Vec<Event>) {
    out.push(Event {
        kind: node.marks.text(),
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

fn marks_in(path: &str, text: &str) -> Vec<make::Mark> {
    let extension = Path::new(path).extension().and_then(|e| e.to_str());
    if extension == Some("md") {
        return tracelean_core::surface::highlight::markdown(text);
    }
    let lang = extension.and_then(Lang::from_extension);
    tracelean_core::surface::highlight::tokens(text, lang)
}
