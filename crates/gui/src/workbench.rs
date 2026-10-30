use std::cell::Cell;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clrs_core::{Cached, EntryKind, NodeId, Progress, Source};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::{Input, InputEvent, InputState};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::tooltip::Tooltip;
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, Size, StyledExt as _,
    WindowExt as _, h_flex, v_flex,
};
use gpui_kit::{
    App, AppContext as _, ClickEvent, ClipboardItem, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Pixels, Render,
    Role, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, canvas, div,
    px, size,
};

use crate::actions::{
    Back, CancelSearch, ChooseFolder, ClearStaged, CopyPath, Find, Forward, FreshScan, OpenFolder,
    OpenLargestFiles, OpenSettings, ParentFolder, Refresh, Reveal, ReviewChanges, ScanHome,
    ShowIssues, StageSelection, StopScan, ToggleInspector, ToggleSelectionMode, TrashSelection,
    UnstageSelection,
};
use crate::explore::Directory;
use crate::jobs::{Event, Jobs, ScanRequest};
use crate::selection::SelectionState;
use crate::settings::{Appearance, Density, Loaded, Settings};
use crate::{format, panels, recommendations, search, theme, treemap};
use std::sync::Arc;

struct Running {
    progress: Arc<Progress>,
    started: Instant,
}

#[derive(Clone, Copy)]
enum SearchScope {
    Folder,
    Scan,
}

/// What the table currently lists instead of a folder's children.
struct Results {
    title: String,
    total: usize,
}

pub(crate) struct Workbench {
    pub(crate) settings: Settings,
    jobs: Jobs,
    root: Option<PathBuf>,
    running: Option<Running>,
    table: Option<Entity<TableState<Directory>>>,
    table_subscription: Option<Subscription>,
    selection: SelectionState,
    inspector: bool,
    treemap: bool,
    treemap_size: Rc<Cell<gpui_kit::Size<Pixels>>>,
    history: Vec<NodeId>,
    history_pos: usize,
    search: Entity<InputState>,
    search_scope: SearchScope,
    search_task: Option<Task<()>>,
    search_cancel: Arc<AtomicBool>,
    results: Option<Results>,
    groups: Vec<recommendations::Group>,
    analysis: Option<Task<()>>,
    cleaning: Option<Task<()>>,
    cleaned: Option<String>,
    status: String,
    warning: Option<String>,
    failed: bool,
    focus: FocusHandle,
    saving: Option<Task<()>>,
}

impl Workbench {
    pub(crate) fn new(
        loaded: Loaded,
        jobs: Jobs,
        root: Option<PathBuf>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        cx.observe_window_appearance(window, |this, _, cx| {
            if this.settings.appearance == Appearance::System {
                theme::apply(Appearance::System, cx);
            }
        })
        .detach();
        cx.observe_window_bounds(window, |this, window, _| {
            let bounds = window.window_bounds().get_bounds();
            this.settings.window = Some(crate::settings::WindowBounds {
                x: bounds.origin.x.into(),
                y: bounds.origin.y.into(),
                width: bounds.size.width.into(),
                height: bounds.size.height.into(),
            });
        })
        .detach();
        cx.on_app_quit(|this, cx| {
            this.jobs.cancel();
            let settings = this.settings.clone();
            let previous = this.saving.take();
            cx.background_executor().spawn(async move {
                if let Some(previous) = previous {
                    previous.await;
                }
                if let Err(error) = settings.save() {
                    eprintln!("Could not save settings: {error}");
                }
            })
        })
        .detach();
        window.on_window_should_close(cx, |_, cx| {
            cx.quit();
            false
        });
        cx.spawn_in(window, async move |view, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(100))
                    .await;
                if view.update_in(cx, Self::poll).is_err() {
                    break;
                }
            }
        })
        .detach();
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search names"));
        cx.subscribe_in(&search, window, |this, _, event, window, cx| {
            if matches!(event, InputEvent::Change | InputEvent::PressEnter { .. }) {
                this.run_search(window, cx);
            }
        })
        .detach();
        let mut this = Self {
            settings: loaded.settings,
            jobs,
            root: None,
            running: None,
            table: None,
            table_subscription: None,
            selection: SelectionState::default(),
            inspector: true,
            treemap: false,
            treemap_size: Rc::new(Cell::new(size(px(0.), px(0.)))),
            history: Vec::new(),
            history_pos: 0,
            search,
            search_scope: SearchScope::Folder,
            search_task: None,
            search_cancel: Arc::new(AtomicBool::new(false)),
            results: None,
            groups: Vec::new(),
            analysis: None,
            cleaning: None,
            cleaned: None,
            status: "Choose a folder to see what is taking up space.".into(),
            warning: loaded.warning,
            failed: false,
            focus,
            saving: None,
        };
        if let Some(root) = root {
            this.scan(root, false, cx);
        }
        this
    }

    pub(crate) fn persist(&mut self, cx: &mut Context<Self>) {
        let settings = self.settings.clone();
        let previous = self.saving.take();
        self.saving = Some(cx.spawn(async move |view, cx| {
            if let Some(previous) = previous {
                previous.await;
            }
            let result = cx
                .background_executor()
                .spawn(async move { settings.save() })
                .await;
            if let Err(error) = result {
                let _ = view.update(cx, |this, cx| {
                    this.warning = Some(format!("Settings could not be saved: {error}"));
                    cx.notify();
                });
            }
        }));
    }

    fn scan(&mut self, root: PathBuf, fresh: bool, cx: &mut Context<Self>) {
        let request = ScanRequest {
            root: root.clone(),
            use_cache: self.settings.cache_enabled && !self.settings.cross_devices,
            fresh,
            cross_devices: self.settings.cross_devices,
        };
        match self.jobs.start(request) {
            Ok(started) => {
                debug_assert_eq!(started.generation, self.jobs.generation());
                self.running = Some(Running {
                    progress: started.progress,
                    started: Instant::now(),
                });
                self.root = Some(root);
                self.failed = false;
                self.warning = None;
                self.status = "Scanning…".into();
            }
            Err(error) => {
                self.warning = Some(format!("{error}. Quit and reopen cl-rs to retry."));
                self.failed = true;
            }
        }
        cx.notify();
    }

    fn poll(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        while let Some(completion) = self.jobs.poll() {
            if completion.generation != self.jobs.generation() {
                continue;
            }
            match completion.event {
                Event::Finished { cached, elapsed } => {
                    if self
                        .running
                        .as_ref()
                        .is_some_and(|running| running.progress.is_cancelled())
                    {
                        self.running = None;
                        self.status =
                            "Scan stopped. Previous results have not been changed.".into();
                        cx.notify();
                        continue;
                    }
                    self.running = None;
                    self.failed = false;
                    let source = match cached.source {
                        Source::Scan => "Full scan",
                        Source::Cache { .. } => "Updated saved scan",
                    };
                    self.status = format!(
                        "{source} · {:.1}s · {} files · {} allocated",
                        elapsed.as_secs_f64(),
                        format::count(cached.tree.node(cached.tree.root()).files()),
                        format::allocated(cached.tree.node(cached.tree.root()).size())
                    );
                    if let Some(cleaned) = self.cleaned.take() {
                        self.status = format!("{cleaned} · {}", self.status);
                    }
                    self.settings.remember(cached.tree.root_path());
                    self.root = Some(cached.tree.root_path().to_path_buf());
                    let directory = cached.tree.root();
                    self.history.clear();
                    self.selection.staged.clear();
                    self.analyse(Arc::clone(&cached), cx);
                    self.navigate(cached, directory, false, window, cx);
                    self.persist(cx);
                }
                Event::Cancelled => {
                    self.running = None;
                    self.status =
                        "Scan stopped. Any previous result is still shown; refresh to try again."
                            .into();
                }
                Event::Failed(error) => {
                    self.running = None;
                    self.failed = true;
                    self.warning = Some(format!(
                        "Could not scan: {error}. Check that the folder is available and readable, then retry."
                    ));
                    self.status = "Scan failed. Previous results have not been changed.".into();
                }
                Event::SaveFailed(error) => {
                    self.warning = Some(format!(
                        "Scan complete, but its saved copy could not be written: {error}. Refresh still works."
                    ));
                }
                Event::Saved => {}
            }
            cx.notify();
        }
        if self.running.is_some() {
            cx.notify();
        }
    }

    fn navigate(
        &mut self,
        cached: Arc<Cached>,
        directory: NodeId,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.history.get(self.history_pos) != Some(&directory) {
            if !self.history.is_empty() {
                self.history.truncate(self.history_pos + 1);
            }
            self.history.push(directory);
            self.history_pos = self.history.len() - 1;
        }
        self.open_folder(cached, directory, focus, window, cx);
    }

    fn travel(&mut self, forward: bool, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = &self.table else {
            return;
        };
        let target = if forward {
            Some(self.history_pos + 1)
        } else {
            self.history_pos.checked_sub(1)
        };
        if let Some((position, &directory)) =
            target.and_then(|position| Some((position, self.history.get(position)?)))
        {
            let cached = Arc::clone(&table.read(cx).delegate().cached);
            self.history_pos = position;
            self.open_folder(cached, directory, true, window, cx);
        }
    }

    fn open_folder(
        &mut self,
        cached: Arc<Cached>,
        directory: NodeId,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.cancel_search(cx);
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.results = None;
        self.install(Directory::new(cached, directory), focus, window, cx);
    }

    fn install(
        &mut self,
        mut delegate: Directory,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        delegate.staged.clone_from(&self.selection.staged);
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .col_movable(false)
                .sortable(false)
                .row_header(false)
        });
        self.table_subscription =
            Some(
                cx.subscribe_in(&table, window, |this, table, event, window, cx| {
                    let directory = table.read(cx).delegate();
                    match event {
                        TableEvent::SelectRow(row) => {
                            if let Some(id) = directory.row(*row) {
                                let modifiers = window.modifiers();
                                if modifiers.shift {
                                    this.selection.select_range(id, directory);
                                } else if modifiers.control {
                                    this.selection.toggle(id);
                                } else {
                                    this.selection.select_single(id);
                                }
                            }
                        }
                        TableEvent::DoubleClickedRow(row) => {
                            if let Some(id) = table.read(cx).delegate().row(*row) {
                                this.selection.select_single(id);
                                this.open_selected(window, cx);
                            }
                        }
                        TableEvent::ClearSelection => this.selection.clear(),
                        _ => {}
                    }
                    cx.notify();
                }),
            );
        let replace_focused = self
            .table
            .as_ref()
            .is_some_and(|old| old.focus_handle(cx).contains_focused(window, cx));
        if focus || replace_focused || self.focus.is_focused(window) {
            window.focus(&table.focus_handle(cx), cx);
        }
        self.table = Some(table);
        self.selection.clear();
        cx.notify();
    }

    fn cancel_search(&mut self, _: &mut Context<Self>) {
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_task = None;
    }

    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_owned();
        self.cancel_search(cx);
        let Some(table) = &self.table else {
            return;
        };
        let directory = table.read(cx).delegate();
        let (cached, folder) = (Arc::clone(&directory.cached), directory.id);
        if query.is_empty() {
            if self.results.take().is_some() {
                self.install(Directory::new(cached, folder), false, window, cx);
            }
            return;
        }
        let scope = match self.search_scope {
            SearchScope::Folder => folder,
            SearchScope::Scan => cached.tree.root(),
        };
        let cancel = Arc::new(AtomicBool::new(false));
        self.search_cancel = Arc::clone(&cancel);
        self.search_task = Some(cx.spawn_in(window, async move |view, cx| {
            cx.background_executor()
                .timer(Duration::from_millis(150))
                .await;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let (data, needle, flag) = (Arc::clone(&cached), query.clone(), Arc::clone(&cancel));
            let found = cx
                .background_executor()
                .spawn(async move { search::find(&data, scope, &needle, &flag) })
                .await;
            if cancel.load(Ordering::Relaxed) {
                return;
            }
            let _ = view.update_in(cx, |this, window, cx| {
                let title = format!("Results for “{query}”");
                this.show_results(cached, folder, found.ids, title, found.total, window, cx);
            });
        }));
    }

    #[expect(clippy::too_many_arguments)]
    fn show_results(
        &mut self,
        cached: Arc<Cached>,
        folder: NodeId,
        rows: Vec<NodeId>,
        title: String,
        total: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.results = Some(Results { title, total });
        self.install(Directory::results(cached, folder, rows), false, window, cx);
    }

    fn close_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_search(cx);
        let had_results = self.results.is_some();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        if let Some(table) = &self.table
            && had_results
        {
            let directory = table.read(cx).delegate();
            let (cached, folder) = (Arc::clone(&directory.cached), directory.id);
            self.results = None;
            self.install(Directory::new(cached, folder), true, window, cx);
        }
    }

    fn largest_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = &self.table else {
            return;
        };
        let directory = table.read(cx).delegate();
        let (cached, folder) = (Arc::clone(&directory.cached), directory.id);
        let rows = cached.tree.largest_files(search::LIMIT);
        let total = rows.len();
        self.show_results(
            cached,
            folder,
            rows,
            "Largest files".into(),
            total,
            window,
            cx,
        );
    }

    fn analyse(&mut self, cached: Arc<Cached>, cx: &mut Context<Self>) {
        self.groups.clear();
        self.analysis = Some(cx.spawn(async move |view, cx| {
            let groups = cx
                .background_executor()
                .spawn(async move { recommendations::find(&cached) })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.groups = groups;
                cx.notify();
            });
        }));
    }

    fn show_group(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(table), Some(group)) = (&self.table, self.groups.get(index)) else {
            return;
        };
        let directory = table.read(cx).delegate();
        let (cached, folder) = (Arc::clone(&directory.cached), directory.id);
        let (rows, title) = (group.items.clone(), group.name.clone());
        let total = rows.len();
        self.cancel_search(cx);
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.show_results(cached, folder, rows, title, total, window, cx);
    }

    fn sync_staged(&mut self, cx: &mut Context<Self>) {
        if let Some(table) = &self.table {
            let staged = self.selection.staged.clone();
            table.update(cx, |table, cx| {
                table.delegate_mut().staged = staged;
                cx.notify();
            });
        }
        cx.notify();
    }

    fn stage_rows(&mut self, cx: &mut Context<Self>) {
        if let Some(table) = &self.table {
            let rows = table.read(cx).delegate().rows.clone();
            self.selection.staged.extend(rows);
            self.sync_staged(cx);
        }
    }

    /// Staged items worth reviewing: what each is, its size, and the command that replaces
    /// moving it to the Trash.
    pub(crate) fn review(&self, cx: &App) -> Vec<panels::ReviewItem> {
        let Some(table) = &self.table else {
            return Vec::new();
        };
        let tree = &table.read(cx).delegate().cached.tree;
        self.selection
            .staged_roots(tree)
            .into_iter()
            .map(|id| panels::ReviewItem {
                id,
                target: tree.path(id),
                path: format::path(&tree.path(id)),
                size: tree.node(id).size(),
                command: self
                    .groups
                    .iter()
                    .find(|group| group.items.contains(&id))
                    .and_then(|group| group.command.clone()),
            })
            .collect()
    }

    fn staged_summary(&self, cx: &App) -> Option<String> {
        let table = self.table.as_ref()?;
        if self.selection.staged.is_empty() {
            return None;
        }
        let tree = &table.read(cx).delegate().cached.tree;
        let roots = self.selection.staged_roots(tree);
        let total: u64 = roots.iter().map(|&id| tree.node(id).size()).sum();
        Some(format!(
            "{} staged · {}",
            roots.len(),
            format::allocated(total)
        ))
    }

    pub(crate) fn can_clean(&self) -> bool {
        self.running.is_none() && self.cleaning.is_none()
    }

    /// Cleans every staged item: runs its rule's command if installed, otherwise moves it
    /// to the Trash, then rescans so the view reflects what changed.
    pub(crate) fn clean_staged(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let items = self.review(cx);
        if items.is_empty() || !self.can_clean() {
            return;
        }
        window.close_sheet(cx);
        self.status = format!("Cleaning {} items…", items.len());
        let commands: Vec<Option<Vec<String>>> =
            items.iter().map(|item| item.command.clone()).collect();
        self.cleaning = Some(cx.spawn(async move |view, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move {
                    items
                        .into_iter()
                        .zip(commands)
                        .map(|(item, command)| {
                            let result = clrs_core::clean_target(&item.target, command.as_deref());
                            (item, result)
                        })
                        .collect::<Vec<_>>()
                })
                .await;
            let _ = view.update(cx, |this, cx| this.finish_clean(&outcomes, cx));
        }));
        cx.notify();
    }

    fn finish_clean(
        &mut self,
        outcomes: &[(
            panels::ReviewItem,
            Result<clrs_core::Outcome, clrs_core::Error>,
        )],
        cx: &mut Context<Self>,
    ) {
        self.cleaning = None;
        let mut failures = Vec::new();
        let mut freed = 0;
        for (item, result) in outcomes {
            match result {
                Ok(_) => {
                    self.selection.staged.remove(&item.id);
                    freed += item.size;
                }
                Err(error) => failures.push(format!("{}: {error}", item.path)),
            }
        }
        let done = outcomes.len() - failures.len();
        self.cleaned = Some(format!(
            "Cleaned {done} of {} items ({}; empty the Trash to free it)",
            outcomes.len(),
            format::allocated(freed)
        ));
        self.sync_staged(cx);
        self.refresh(false, cx);
        if !failures.is_empty() {
            self.warning = Some(format!(
                "{} items could not be cleaned and stay staged. {}",
                failures.len(),
                failures.join("; ")
            ));
        }
        cx.notify();
    }

    pub(crate) fn clear_staged(&mut self, cx: &mut Context<Self>) {
        self.selection.staged.clear();
        self.sync_staged(cx);
    }

    fn choose(window: &mut Window, cx: &mut Context<Self>) {
        let result = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: false,
            prompt: Some("Scan Folder".into()),
        });
        cx.spawn_in(window, async move |view, cx| match result.await {
            Ok(Ok(Some(paths))) => {
                if let Some(path) = paths.into_iter().next() {
                    let _ = view.update(cx, |this, cx| this.scan(path, false, cx));
                }
            }
            Ok(Err(error)) => {
                let _ = view.update(cx, |this, cx| {
                    this.warning = Some(format!("Could not open the folder picker: {error}"));
                    cx.notify();
                });
            }
            _ => {}
        })
        .detach();
    }

    fn refresh(&mut self, fresh: bool, cx: &mut Context<Self>) {
        if let Some(root) = self.root.clone() {
            self.scan(root, fresh, cx);
        }
    }

    fn home(&mut self, cx: &mut Context<Self>) {
        if let Some(home) = std::env::home_dir() {
            self.scan(home, false, cx);
        } else {
            self.warning = Some("No home folder was found. Use Choose Folder instead.".into());
            cx.notify();
        }
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(table), Some(&id)) = (&self.table, self.selection.selected.last()) {
            let directory = table.read(cx).delegate();
            let cached = Arc::clone(&directory.cached);
            if cached.tree.node(id).kind() == EntryKind::Dir {
                self.navigate(cached, id, true, window, cx);
            }
        }
    }

    fn pick_tile(
        &mut self,
        row: usize,
        clicks: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(table) = self.table.clone() else {
            return;
        };
        if clicks >= 2 {
            if let Some(id) = table.read(cx).delegate().row(row) {
                self.selection.select_single(id);
                self.open_selected(window, cx);
            }
        } else {
            table.update(cx, |table, cx| table.set_selected_row(row, cx));
        }
    }

    fn parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(table) = &self.table {
            let directory = table.read(cx).delegate();
            if let Some(parent) = directory.cached.tree.parent(directory.id) {
                self.navigate(Arc::clone(&directory.cached), parent, true, window, cx);
            }
        }
    }

    fn selection_path(&self, cx: &Context<Self>) -> Option<PathBuf> {
        let directory = self.table.as_ref()?.read(cx).delegate();
        let id = self
            .selection
            .selected
            .last()
            .copied()
            .unwrap_or(directory.id);
        Some(directory.cached.tree.path(id))
    }

    fn reveal(&self, cx: &mut Context<Self>) {
        if let Some(path) = self.selection_path(cx) {
            cx.reveal_path(&path);
        }
    }

    fn copy_path(&mut self, cx: &mut Context<Self>) {
        if let Some(path) = self.selection_path(cx) {
            if let Some(path) = path.to_str() {
                cx.write_to_clipboard(ClipboardItem::new_string(path.into()));
            } else {
                self.warning = Some("This path contains non-Unicode bytes and cannot be copied exactly as text. Use Reveal in Finder.".into());
                cx.notify();
            }
        }
    }

    fn issues(&self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(table) = &self.table {
            panels::issues(Arc::clone(&table.read(cx).delegate().cached), window, cx);
        }
    }

    #[allow(clippy::too_many_lines)]
    fn treemap_panel(&self, directory: &Directory, cx: &Context<Self>) -> impl IntoElement + use<> {
        const HEIGHT: f32 = 260.;
        let tree = &directory.cached.tree;
        let dark = cx.theme().is_dark();
        let measured = self.treemap_size.get();
        let sizes: Vec<u64> = directory
            .rows
            .iter()
            .map(|&id| tree.node(id).size())
            .collect();
        let tiles = treemap::layout(
            &sizes,
            f32::from(measured.width),
            f32::from(measured.height),
        );
        let hidden = directory.rows.len() - tiles.len();
        let measure = Rc::clone(&self.treemap_size);
        let view = cx.weak_entity();
        let mut area = div().relative().flex_1().min_h_0().overflow_hidden().child(
            canvas(
                move |bounds, _, cx| {
                    if measure.get() != bounds.size {
                        measure.set(bounds.size);
                        let view = view.clone();
                        cx.defer(move |cx| {
                            let _ = view.update(cx, |_, cx| cx.notify());
                        });
                    }
                },
                |_, (), _, _| {},
            )
            .absolute()
            .size_full(),
        );
        for tile in &tiles {
            let row = tile.row;
            let id = directory.rows[row];
            let node = tree.node(id);
            let name = tree.name(id).to_string_lossy().into_owned();
            let allocated = format::allocated(node.size());
            let label = format!("{name}, {allocated}");
            let color = treemap::fill(row, node.kind() == EntryKind::Dir, dark);
            let selected = self.selection.is_selected(id);
            let tooltip = format!(
                "{name}\n{allocated} · {}",
                format::kind(node.kind(), tree.name(id))
            );
            let mut cell = div()
                .id(("tile", row))
                .role(Role::ListItem)
                .aria_label(label)
                .absolute()
                .left(px(tile.x + 0.5))
                .top(px(tile.y + 0.5))
                .w(px((tile.width - 1.).max(1.)))
                .h(px((tile.height - 1.).max(1.)))
                .p_1()
                .overflow_hidden()
                .rounded(px(3.))
                .border_2()
                .border_color(if selected { cx.theme().primary } else { color })
                .bg(color)
                .cursor_pointer()
                .hover(|style| style.opacity(0.8))
                .tooltip(move |window, cx| Tooltip::new(tooltip.clone()).build(window, cx))
                .on_click(cx.listener(move |this, event: &ClickEvent, window, cx| {
                    this.pick_tile(row, event.click_count(), window, cx);
                }));
            if tile.width >= 72. && tile.height >= 36. {
                cell = cell
                    .child(div().truncate().font_semibold().child(name))
                    .child(
                        div()
                            .truncate()
                            .text_color(cx.theme().muted_foreground)
                            .child(allocated),
                    );
            }
            area = area.child(cell);
        }
        if tiles.is_empty() {
            area = area.child(
                div()
                    .p_4()
                    .text_color(cx.theme().muted_foreground)
                    .child("Nothing with allocated space in this folder."),
            );
        }
        let mut panel = v_flex()
            .h(px(HEIGHT))
            .flex_shrink_0()
            .p_2()
            .gap_1()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(area);
        if hidden > 0 {
            panel = panel.child(div().text_color(cx.theme().muted_foreground).child(format!(
                "{} smaller items are listed below but not drawn.",
                format::count(hidden as u64)
            )));
        }
        panel
    }

    #[allow(clippy::too_many_lines)]
    fn sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let current_root = &self.root;
        let mut sidebar = v_flex()
            .w(px(theme::SIDEBAR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .p_3()
            .gap_2()
            .bg(cx.theme().sidebar)
            .border_r_1()
            .border_color(cx.theme().border)
            .child(
                div()
                    .px_2()
                    .py_3()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child("Locations"),
            )
            .child(
                h_flex().w_full().justify_start().child(
                    Button::new("home")
                        .label("Home")
                        .ghost()
                        .w_full()
                        .selected(
                            current_root
                                .as_ref()
                                .is_some_and(|r| r == std::path::Path::new("~")),
                        )
                        .on_click(cx.listener(|this, _, _, cx| this.home(cx))),
                ),
            )
            .child(
                h_flex().w_full().justify_start().child(
                    Button::new("choose-sidebar")
                        .label("Choose Folder…")
                        .ghost()
                        .w_full()
                        .on_click(cx.listener(|_, _, window, cx| Self::choose(window, cx))),
                ),
            )
            .child(
                h_flex().w_full().justify_start().child(
                    Button::new("largest-files")
                        .label("Largest Files")
                        .ghost()
                        .w_full()
                        .disabled(self.table.is_none())
                        .on_click(
                            cx.listener(|this, _, window, cx| this.largest_files(window, cx)),
                        ),
                ),
            );
        if !self.groups.is_empty() {
            sidebar = sidebar.child(
                div()
                    .px_2()
                    .pt_5()
                    .pb_1()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child("Recommendations"),
            );
            for (index, group) in self.groups.iter().take(6).enumerate() {
                let note = match group.safety {
                    clrs_core::Safety::Safe => "safe to clean",
                    clrs_core::Safety::Review => "review first",
                };
                sidebar = sidebar.child(
                    v_flex()
                        .id(("recommendation", index))
                        .role(Role::Button)
                        .aria_label(format!(
                            "{}, {}, {note}",
                            group.name,
                            format::allocated(group.size)
                        ))
                        .px_2()
                        .py_1()
                        .rounded(px(6.))
                        .cursor_pointer()
                        .hover(|style| style.bg(cx.theme().sidebar_accent))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.show_group(index, window, cx);
                        }))
                        .child(div().truncate().child(group.name.clone()))
                        .child(
                            div()
                                .truncate()
                                .text_color(cx.theme().muted_foreground)
                                .child(format!("{} · {note}", format::allocated(group.size))),
                        ),
                );
            }
        }
        if !self.settings.recent.is_empty() {
            sidebar = sidebar.child(
                div()
                    .px_2()
                    .pt_5()
                    .pb_1()
                    .font_semibold()
                    .text_color(cx.theme().foreground)
                    .child("Recent folders"),
            );
            for (index, root) in self.settings.recent.iter().enumerate() {
                let root = root.clone();
                let label = format::name(&root);
                sidebar = sidebar.child(
                    h_flex().w_full().justify_start().child(
                        Button::new(("recent", index))
                            .label(label)
                            .tooltip(format::path(&root))
                            .ghost()
                            .w_full()
                            .selected(current_root.as_ref().is_some_and(|r| r == &root))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.scan(root.clone(), false, cx);
                            })),
                    ),
                );
            }
        }
        sidebar.child(div().flex_1()).child(
            h_flex().w_full().justify_start().child(
                Button::new("settings")
                    .label("Settings…")
                    .ghost()
                    .w_full()
                    .on_click(cx.listener(|_, _, window, cx| panels::settings(window, cx))),
            ),
        )
    }

    fn inspector(&self, cx: &Context<Self>) -> impl IntoElement + use<> {
        let mut panel = v_flex()
            .id("inspector")
            .overflow_y_scroll()
            .w(px(theme::INSPECTOR_WIDTH))
            .flex_shrink_0()
            .h_full()
            .p_5()
            .gap_4()
            .border_l_1()
            .border_color(cx.theme().border);
        if let Some(table) = &self.table {
            panel = panel.child(div().font_semibold().child("Inspector"));
            let directory = table.read(cx).delegate();
            let id = self
                .selection
                .selected
                .last()
                .copied()
                .unwrap_or(directory.id);
            let node = directory.cached.tree.node(id);
            panel = panel
                .child(
                    div()
                        .font_semibold()
                        .line_clamp(2)
                        .child(format::name(&directory.cached.tree.path(id))),
                )
                .child(panels::detail(
                    "Allocated size",
                    format!("{} ({} bytes)", format::allocated(node.size()), node.size()),
                    cx,
                ))
                .child(panels::detail(
                    "Kind",
                    format::kind(node.kind(), directory.cached.tree.name(id)).into(),
                    cx,
                ));
            if node.kind() == EntryKind::Dir {
                panel = panel.child(panels::detail("Files", format::count(node.files()), cx));
            }
            panel = panel.child(panels::detail("Path", format::path(&directory.cached.tree.path(id)), cx))
                .child(Button::new("reveal").label("Reveal in Finder").primary().on_click(cx.listener(|this, _, _, cx| this.reveal(cx))))
                .child(Button::new("copy-path").label("Copy Path").on_click(cx.listener(|this, _, _, cx| this.copy_path(cx))))
                .child(div().text_color(cx.theme().muted_foreground).child("Sizes use allocated disk space. Hard links count once; symlinks are not followed."));
            if self.selection.has_selection() {
                let staged = self.selection.is_staged(id);
                panel = panel.child(
                    Button::new("stage")
                        .label(if staged {
                            "Unstage"
                        } else {
                            "Stage for Cleanup"
                        })
                        .on_click(cx.listener(move |this, _, _, cx| {
                            if staged {
                                this.selection.unstage_all();
                            } else {
                                this.selection.stage_all();
                            }
                            this.sync_staged(cx);
                        })),
                );
            }
        }
        panel
    }

    fn toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scanning = self.running.is_some();
        let mut toolbar = h_flex()
            .h(px(56.))
            .px_4()
            .gap_2()
            .flex_shrink_0()
            .border_b_1()
            .border_color(cx.theme().border)
            .child(div().font_semibold().child("Explore"))
            .child(div().w(px(12.)))
            .child(Input::new(&self.search).w(px(240.)).cleanable(true))
            .child(
                Button::new("search-scope")
                    .label(match self.search_scope {
                        SearchScope::Folder => "This folder",
                        SearchScope::Scan => "Entire scan",
                    })
                    .ghost()
                    .disabled(self.table.is_none())
                    .on_click(cx.listener(|this, _, window, cx| {
                        this.search_scope = match this.search_scope {
                            SearchScope::Folder => SearchScope::Scan,
                            SearchScope::Scan => SearchScope::Folder,
                        };
                        this.run_search(window, cx);
                        cx.notify();
                    })),
            )
            .child(div().flex_1())
            .child(
                Button::new("choose-folder")
                    .label("Choose Folder…")
                    .on_click(cx.listener(|_, _, window, cx| Self::choose(window, cx))),
            )
            .child(
                Button::new("refresh")
                    .label(if self.failed { "Retry" } else { "Refresh" })
                    .disabled(self.root.is_none() || scanning)
                    .on_click(cx.listener(|this, _, _, cx| this.refresh(false, cx))),
            );
        if scanning {
            toolbar = toolbar.child(Button::new("stop").label("Stop").on_click(cx.listener(
                |this, _, _, cx| {
                    this.jobs.cancel();
                    cx.notify();
                },
            )));
        }
        toolbar = toolbar.child(
            Button::new("toggle-inspector")
                .label(if self.inspector {
                    "Hide Inspector"
                } else {
                    "Show Inspector"
                })
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.inspector = !this.inspector;
                    cx.notify();
                })),
        );
        toolbar = toolbar.child(
            Button::new("toggle-treemap")
                .label(if self.treemap {
                    "Hide Treemap"
                } else {
                    "Show Treemap"
                })
                .ghost()
                .on_click(cx.listener(|this, _, _, cx| {
                    this.treemap = !this.treemap;
                    cx.notify();
                })),
        );
        if !self.selection.staged.is_empty() {
            toolbar = toolbar.child(
                Button::new("review")
                    .label(format!("Review ({})", self.selection.staged.len()))
                    .primary()
                    .on_click(cx.listener(|_, _, window, cx| panels::review(window, cx))),
            );
        }
        toolbar
    }

    #[allow(clippy::too_many_lines)]
    fn content(&self, cx: &mut Context<Self>) -> impl IntoElement + use<> {
        let scanning = self.running.is_some();
        let mut content = v_flex().flex_1().min_w_0().min_h_0();
        if let Some(warning) = &self.warning {
            content = content.child(
                div()
                    .p_3()
                    .text_color(cx.theme().danger)
                    .child(warning.clone()),
            );
        }
        if let Some(running) = &self.running {
            let progress = running.progress.snapshot();
            let title = if running.progress.is_cancelled() {
                "Stopping…"
            } else {
                "Scanning…"
            };
            content = content.child(v_flex().p_4().gap_1().bg(cx.theme().muted)
                .child(div().font_semibold().child(format!("{title} {}", self.root.as_deref().map_or_else(String::new, format::path))))
                .child(format!("{} files · {} folders · {} found · {:.0}s", format::count(progress.files), format::count(progress.dirs), format::allocated(progress.bytes), running.started.elapsed().as_secs_f64()))
                .child(div().text_color(cx.theme().muted_foreground).child(if self.table.is_some() { "Showing the previous scan below until the new result is ready." } else { "The file list will appear when scanning finishes. You can stop at any time." })));
        }
        if let Some(table) = &self.table {
            let directory = table.read(cx).delegate();
            let cached = Arc::clone(&directory.cached);
            let mut ancestors = Vec::new();
            let mut current = Some(directory.id);
            while let Some(id) = current {
                ancestors.push(id);
                current = cached.tree.parent(id);
            }
            ancestors.reverse();
            let mut breadcrumb = h_flex()
                .px_3()
                .h(px(44.))
                .gap_1()
                .min_w_0()
                .overflow_hidden()
                .child(
                    Button::new("back")
                        .label("Back")
                        .small()
                        .ghost()
                        .disabled(self.history_pos == 0)
                        .on_click(
                            cx.listener(|this, _, window, cx| this.travel(false, window, cx)),
                        ),
                )
                .child(
                    Button::new("forward")
                        .label("Forward")
                        .small()
                        .ghost()
                        .disabled(self.history_pos + 1 >= self.history.len())
                        .on_click(cx.listener(|this, _, window, cx| this.travel(true, window, cx))),
                )
                .child(
                    Button::new("parent")
                        .label("Up")
                        .small()
                        .ghost()
                        .disabled(ancestors.len() <= 1)
                        .on_click(cx.listener(|this, _, window, cx| this.parent(window, cx))),
                );
            for (index, &id) in ancestors
                .iter()
                .skip(ancestors.len().saturating_sub(3))
                .enumerate()
            {
                let path = cached.tree.path(id);
                let label = format::name(&path);
                let target = Arc::clone(&cached);
                breadcrumb = breadcrumb.child(
                    Button::new(("crumb", index))
                        .label(label)
                        .tooltip(format::path(&path))
                        .max_w(px(140.))
                        .min_w_0()
                        .small()
                        .ghost()
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.navigate(Arc::clone(&target), id, true, window, cx);
                        })),
                );
            }
            content = content.child(breadcrumb);
            if !cached.issues.is_empty() {
                let label = format!(
                    "{} unreadable paths — view scan issues",
                    format::count(cached.issues.len() as u64)
                );
                content = content.child(
                    Button::new("issues")
                        .label(label)
                        .secondary()
                        .on_click(cx.listener(|this, _, window, cx| this.issues(window, cx))),
                );
            }
            if let Some(results) = &self.results {
                let shown = directory.rows.len();
                let detail = if results.total > shown {
                    format!(
                        "Showing the largest {} of {}",
                        format::count(shown as u64),
                        format::count(results.total as u64)
                    )
                } else {
                    format!("{} items", format::count(shown as u64))
                };
                content = content.child(
                    h_flex()
                        .px_3()
                        .py_2()
                        .gap_2()
                        .bg(cx.theme().muted)
                        .child(
                            div()
                                .id("results-title")
                                .role(Role::Label)
                                .aria_label(results.title.clone())
                                .font_semibold()
                                .child(results.title.clone()),
                        )
                        .child(
                            div()
                                .id("results-detail")
                                .role(Role::Label)
                                .aria_label(detail.clone())
                                .text_color(cx.theme().muted_foreground)
                                .child(detail),
                        )
                        .child(div().flex_1())
                        .child(
                            Button::new("stage-results")
                                .label("Stage All")
                                .small()
                                .on_click(cx.listener(|this, _, _, cx| this.stage_rows(cx))),
                        )
                        .child(
                            Button::new("close-results")
                                .label("Close")
                                .small()
                                .ghost()
                                .on_click(cx.listener(|this, _, window, cx| {
                                    this.close_results(window, cx);
                                })),
                        ),
                );
            } else if self.treemap {
                content = content.child(self.treemap_panel(directory, cx));
            }
            content = content.child(
                div().flex_1().min_h_0().child(
                    DataTable::new(table)
                        .bordered(false)
                        .stripe(true)
                        .with_size(match self.settings.density {
                            Density::Comfortable => Size::Medium,
                            Density::Compact => Size::Small,
                        }),
                ),
            );
        } else if !scanning {
            content = content.child(v_flex().flex_1().justify_center().items_center().gap_4().p_8()
                .child(div().text_2xl().font_semibold().child("See what's using your space."))
                .child(div().max_w(px(400.)).text_center().text_color(cx.theme().muted_foreground).child("Choose a folder to explore its disk usage. Start with Home, or look at a specific project or drive."))
                .child(
                    h_flex()
                        .gap_3()
                        .child(Button::new("home-welcome").label("Scan Home").primary().on_click(cx.listener(|this, _, _window, cx| this.home(cx))))
                        .child(Button::new("start-welcome").label("Choose Folder…").secondary().on_click(cx.listener(|_, _, window, cx| Self::choose(window, cx))))
                )
                .child(div().text_color(cx.theme().muted_foreground).child("Exploring is read-only. Nothing is moved until you review and clean staged items.")));
        }
        content
    }
}

impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let staged = self.staged_summary(cx);
        let toolbar = self.toolbar(cx);
        let content = self.content(cx);
        let mut body = h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(self.sidebar(cx))
            .child(content);
        if self.inspector {
            body = body.child(self.inspector(cx));
        }
        v_flex()
            .id("workbench")
            .accessibility_id("clrs-workbench")
            .role(Role::Group)
            .aria_label("Disk space workbench")
            .key_context("Workbench")
            .track_focus(&self.focus)
            .size_full()
            .text_size(px(13.))
            .bg(cx.theme().background)
            .text_color(cx.theme().foreground)
            .on_action(cx.listener(|_, _: &ChooseFolder, window, cx| Self::choose(window, cx)))
            .on_action(cx.listener(|this, _: &ScanHome, _, cx| this.home(cx)))
            .on_action(cx.listener(|this, _: &Refresh, _, cx| this.refresh(false, cx)))
            .on_action(cx.listener(|this, _: &FreshScan, _, cx| this.refresh(true, cx)))
            .on_action(cx.listener(|this, _: &StopScan, _, cx| {
                this.jobs.cancel();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &ParentFolder, window, cx| this.parent(window, cx)))
            .on_action(
                cx.listener(|this, _: &OpenFolder, window, cx| this.open_selected(window, cx)),
            )
            .on_action(cx.listener(|this, _: &Reveal, _, cx| this.reveal(cx)))
            .on_action(cx.listener(|this, _: &CopyPath, _, cx| this.copy_path(cx)))
            .on_action(cx.listener(|this, _: &ToggleInspector, _, cx| {
                this.inspector = !this.inspector;
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &OpenSettings, window, cx| panels::settings(window, cx)))
            .on_action(cx.listener(|this, _: &ShowIssues, window, cx| this.issues(window, cx)))
            .on_action(cx.listener(|this, _: &ToggleSelectionMode, _, cx| {
                this.selection.selected.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TrashSelection, window, cx| {
                this.selection.stage_all();
                this.sync_staged(cx);
                panels::review(window, cx);
            }))
            .on_action(cx.listener(|this, _: &StageSelection, _, cx| {
                this.selection.stage_all();
                this.sync_staged(cx);
            }))
            .on_action(cx.listener(|this, _: &UnstageSelection, _, cx| {
                this.selection.unstage_all();
                this.sync_staged(cx);
            }))
            .on_action(cx.listener(|this, _: &ClearStaged, _, cx| this.clear_staged(cx)))
            .on_action(cx.listener(|this, _: &Back, window, cx| this.travel(false, window, cx)))
            .on_action(cx.listener(|this, _: &Forward, window, cx| this.travel(true, window, cx)))
            .on_action(cx.listener(|this, _: &Find, window, cx| {
                this.search.update(cx, |input, cx| input.focus(window, cx));
            }))
            .on_action(cx.listener(|this, _: &CancelSearch, window, cx| {
                this.close_results(window, cx);
            }))
            .on_action(cx.listener(|this, _: &OpenLargestFiles, window, cx| {
                this.largest_files(window, cx);
            }))
            .on_action(cx.listener(|_, _: &ReviewChanges, window, cx| panels::review(window, cx)))
            .child(toolbar)
            .child(body)
            .child(
                h_flex()
                    .id("scan-status")
                    .role(Role::Status)
                    .aria_label(staged.as_ref().map_or_else(
                        || self.status.clone(),
                        |staged| format!("{} · {staged}", self.status),
                    ))
                    .h(px(30.))
                    .px_4()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.status.clone())
                    .child(div().flex_1())
                    .children(staged),
            )
    }
}
