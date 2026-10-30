mod chrome;
mod explorer;
mod tray;

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use clrs_core::{Cached, EntryKind, NodeId, Progress, Source, VolumeSpace};
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::table::{TableEvent, TableState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::{
    App, AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Pixels, Render,
    Role, StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, px, size,
};

use crate::actions::{
    Back, CancelSearch, ChooseFolder, ClearStaged, CopyPath, Find, Forward, FreshScan, OpenFolder,
    OpenLargestFiles, OpenSettings, ParentFolder, Refresh, Reveal, ReviewChanges, ScanHome,
    ShowIssues, ShowSuggestions, StageSelection, StopScan, ToggleSelectionMode, TrashSelection,
    UnstageSelection,
};
use crate::explore::{Directory, Hooks};
use crate::jobs::{Event, Jobs, ScanRequest};
use crate::recommendations::Suggestions;
use crate::selection::SelectionState;
use crate::settings::{Appearance, Loaded, Settings};
use crate::{format, panels, recommendations, search, theme};

struct Running {
    progress: Arc<Progress>,
    started: Instant,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum SearchScope {
    Folder,
    Scan,
}

/// What the list currently shows instead of a folder's children.
#[derive(Clone, PartialEq, Eq)]
enum ResultsKind {
    Search,
    Largest,
    Suggestions,
}

struct Results {
    kind: ResultsKind,
    title: String,
    total: usize,
}

/// How the last completed scan was produced.
struct Scanned {
    full: bool,
    elapsed: Duration,
    at: Instant,
}

/// Progress of a clean started from the review sheet.
#[derive(Clone, Debug)]
pub(crate) enum Cleaning {
    Idle,
    Running { total: usize },
    Done(CleanReport),
}

#[derive(Clone, Debug)]
pub(crate) struct CleanReport {
    pub(crate) cleaned: usize,
    pub(crate) freed: u64,
    pub(crate) failures: Vec<String>,
}

pub(crate) struct Workbench {
    pub(crate) settings: Settings,
    jobs: Jobs,
    root: Option<PathBuf>,
    running: Option<Running>,
    scanned: Option<Scanned>,
    table: Option<Entity<TableState<Directory>>>,
    table_subscription: Option<Subscription>,
    selection: SelectionState,
    hovered: Option<NodeId>,
    treemap_size: Rc<Cell<gpui_kit::Size<Pixels>>>,
    treemap_cache: RefCell<Option<explorer::TreemapCache>>,
    list_width: Rc<Cell<Pixels>>,
    history: Vec<NodeId>,
    history_pos: usize,
    search: Entity<InputState>,
    search_scope: SearchScope,
    search_task: Option<Task<()>>,
    search_cancel: Arc<AtomicBool>,
    results: Option<Results>,
    suggestions: Arc<Suggestions>,
    analysis: Option<Task<()>>,
    volume: Option<VolumeSpace>,
    cleaning: Cleaning,
    clean_task: Option<Task<()>>,
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
        let search =
            cx.new(|cx| InputState::new(window, cx).placeholder("Search files and folders"));
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
            scanned: None,
            table: None,
            table_subscription: None,
            selection: SelectionState::default(),
            hovered: None,
            treemap_size: Rc::new(Cell::new(size(px(0.), px(0.)))),
            treemap_cache: RefCell::new(None),
            list_width: Rc::new(Cell::new(px(0.))),
            history: Vec::new(),
            history_pos: 0,
            search,
            search_scope: SearchScope::Folder,
            search_task: None,
            search_cancel: Arc::new(AtomicBool::new(false)),
            results: None,
            suggestions: Arc::new(Suggestions::default()),
            analysis: None,
            volume: None,
            cleaning: Cleaning::Idle,
            clean_task: None,
            status: String::new(),
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
                self.volume = clrs_core::volume_space(&root).ok();
                self.root = Some(root);
                self.failed = false;
                self.warning = None;
                self.status = String::new();
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
                        self.status = "Scan stopped. The previous result is unchanged.".into();
                        cx.notify();
                        continue;
                    }
                    self.running = None;
                    self.failed = false;
                    self.scanned = Some(Scanned {
                        full: matches!(cached.source, Source::Scan),
                        elapsed,
                        at: Instant::now(),
                    });
                    self.status = String::new();
                    self.settings.remember(cached.tree.root_path());
                    self.root = Some(cached.tree.root_path().to_path_buf());
                    let directory = cached.tree.root();
                    self.history.clear();
                    self.selection.staged.clear();
                    self.suggestions = Arc::new(Suggestions::default());
                    self.analyse(Arc::clone(&cached), cx);
                    self.navigate(cached, directory, false, window, cx);
                    self.persist(cx);
                }
                Event::Cancelled => {
                    self.running = None;
                    self.status =
                        "Scan stopped. Any previous result is still shown; rescan to try again."
                            .into();
                }
                Event::Failed(error) => {
                    self.running = None;
                    self.failed = true;
                    self.warning = Some(format!(
                        "Could not scan: {error}. Check that the folder is available and readable, then retry."
                    ));
                    self.status = "Scan failed. The previous result is unchanged.".into();
                }
                Event::SaveFailed(error) => {
                    self.warning = Some(format!(
                        "Scan complete, but its saved copy could not be written: {error}. Rescanning still works."
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

    fn cached(&self, cx: &App) -> Option<Arc<Cached>> {
        Some(Arc::clone(&self.table.as_ref()?.read(cx).delegate().cached))
    }

    fn folder(&self, cx: &App) -> Option<NodeId> {
        Some(self.table.as_ref()?.read(cx).delegate().id)
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
        let Some(cached) = self.cached(cx) else {
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
        self.cancel_search();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        self.results = None;
        let delegate = Directory::new(cached, directory, &self.suggestions, Self::hooks(cx));
        self.install(delegate, focus, window, cx);
    }

    fn hooks(cx: &Context<Self>) -> Hooks {
        let hover = cx.weak_entity();
        let toggle = cx.weak_entity();
        Hooks {
            hover: Rc::new(move |id, hovered, cx| {
                let _ = hover.update(cx, |this, cx| this.hover(id, hovered, cx));
            }),
            toggle: Rc::new(move |id, cx| {
                let _ = toggle.update(cx, |this, cx| this.toggle_staged(id, cx));
            }),
        }
    }

    fn install(
        &mut self,
        mut delegate: Directory,
        focus: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        delegate.staged.clone_from(&self.selection.staged);
        delegate.fit(self.list_width.get());
        let table = cx.new(|cx| {
            TableState::new(delegate, window, cx)
                .col_movable(false)
                .col_resizable(false)
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
                                } else if modifiers.platform || modifiers.control {
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
        self.hovered = None;
        cx.notify();
    }

    /// Re-fits the list's name column after the window or split changed width.
    fn fit_list(&self, cx: &mut Context<Self>) {
        let width = self.list_width.get();
        if let Some(table) = &self.table {
            table.update(cx, |table, cx| {
                if table.delegate_mut().fit(width) {
                    table.refresh(cx);
                    cx.notify();
                }
            });
        }
    }

    fn set_hovered(&mut self, id: Option<NodeId>, cx: &mut Context<Self>) {
        if self.hovered == id {
            return;
        }
        self.hovered = id;
        if let Some(table) = &self.table {
            table.update(cx, |table, cx| {
                table.delegate_mut().hovered = id;
                cx.notify();
            });
        }
        cx.notify();
    }

    fn cancel_search(&mut self) {
        self.search_cancel.store(true, Ordering::Relaxed);
        self.search_task = None;
    }

    fn run_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.search.read(cx).value().trim().to_owned();
        self.cancel_search();
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            return;
        };
        if query.is_empty() {
            if self
                .results
                .as_ref()
                .is_some_and(|results| results.kind == ResultsKind::Search)
            {
                self.results = None;
                let delegate = Directory::new(cached, folder, &self.suggestions, Self::hooks(cx));
                self.install(delegate, false, window, cx);
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
                this.show_results(
                    ResultsKind::Search,
                    cached,
                    folder,
                    found.ids,
                    title,
                    found.total,
                    window,
                    cx,
                );
            });
        }));
    }

    #[expect(clippy::too_many_arguments)]
    fn show_results(
        &mut self,
        kind: ResultsKind,
        cached: Arc<Cached>,
        folder: NodeId,
        rows: Vec<NodeId>,
        title: String,
        total: usize,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.results = Some(Results { kind, title, total });
        let delegate = Directory::results(cached, folder, rows, &self.suggestions, Self::hooks(cx));
        self.install(delegate, false, window, cx);
    }

    fn close_results(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_search();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
        if self.results.take().is_some()
            && let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx))
        {
            let delegate = Directory::new(cached, folder, &self.suggestions, Self::hooks(cx));
            self.install(delegate, true, window, cx);
        }
    }

    fn largest_files(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            return;
        };
        self.reset_search(window, cx);
        let rows = cached.tree.largest_files(search::LIMIT);
        let total = rows.len();
        self.show_results(
            ResultsKind::Largest,
            cached,
            folder,
            rows,
            "Largest files".into(),
            total,
            window,
            cx,
        );
    }

    fn show_suggestions(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx)) else {
            return;
        };
        self.reset_search(window, cx);
        let rows: Vec<NodeId> = self
            .suggestions
            .groups
            .iter()
            .flat_map(|group| group.items.iter().copied())
            .collect();
        let total = rows.len();
        let mut rows = rows;
        rows.sort_by_key(|&id| std::cmp::Reverse(cached.tree.node(id).size()));
        rows.truncate(search::LIMIT);
        self.show_results(
            ResultsKind::Suggestions,
            cached,
            folder,
            rows,
            "Suggestions".into(),
            total,
            window,
            cx,
        );
    }

    fn reset_search(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.cancel_search();
        self.search
            .update(cx, |input, cx| input.set_value("", window, cx));
    }

    fn analyse(&mut self, cached: Arc<Cached>, cx: &mut Context<Self>) {
        self.analysis = Some(cx.spawn(async move |view, cx| {
            let suggestions = cx
                .background_executor()
                .spawn(async move { recommendations::find(&cached) })
                .await;
            let _ = view.update(cx, |this, cx| {
                this.suggestions = Arc::new(suggestions);
                if let Some(table) = &this.table {
                    let suggestions = Arc::clone(&this.suggestions);
                    table.update(cx, |table, cx| {
                        table.delegate_mut().classify(&suggestions);
                        cx.notify();
                    });
                }
                cx.notify();
            });
        }));
    }

    fn toggle_staged(&mut self, id: NodeId, cx: &mut Context<Self>) {
        if !self.selection.staged.remove(&id) {
            self.selection.staged.insert(id);
        }
        self.sync_staged(cx);
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
        let Some(cached) = self.cached(cx) else {
            return Vec::new();
        };
        let tree = &cached.tree;
        self.selection
            .staged_roots(tree)
            .into_iter()
            .map(|id| {
                let group = self.suggestions.of(id);
                panels::ReviewItem {
                    id,
                    target: tree.path(id),
                    path: format::path(&tree.path(id)),
                    name: group.map_or_else(
                        || tree.name(id).to_string_lossy().into_owned(),
                        |group| group.name.clone(),
                    ),
                    description: group.and_then(|group| group.description.clone()),
                    safety: group.map(|group| group.safety),
                    size: tree.node(id).size(),
                    command: group.and_then(|group| group.command.clone()),
                }
            })
            .collect()
    }

    pub(crate) fn cleaning(&self) -> &Cleaning {
        &self.cleaning
    }

    pub(crate) fn can_clean(&self) -> bool {
        self.running.is_none() && !matches!(self.cleaning, Cleaning::Running { .. })
    }

    /// Cleans every staged item: runs its rule's command if installed, otherwise moves it
    /// to the Trash, then rescans so the view reflects what changed.
    pub(crate) fn clean_staged(&mut self, cx: &mut Context<Self>) {
        let items = self.review(cx);
        if items.is_empty() || !self.can_clean() {
            return;
        }
        self.cleaning = Cleaning::Running { total: items.len() };
        self.clean_task = Some(cx.spawn(async move |view, cx| {
            let outcomes = cx
                .background_executor()
                .spawn(async move {
                    items
                        .into_iter()
                        .map(|item| {
                            let result =
                                clrs_core::clean_target(&item.target, item.command.as_deref());
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
        let mut report = CleanReport {
            cleaned: 0,
            freed: 0,
            failures: Vec::new(),
        };
        for (item, result) in outcomes {
            match result {
                Ok(_) => {
                    self.selection.staged.remove(&item.id);
                    report.cleaned += 1;
                    report.freed += item.size;
                }
                Err(error) => report.failures.push(format!("{}: {error}", item.path)),
            }
        }
        self.cleaning = Cleaning::Done(report);
        self.clean_task = None;
        self.sync_staged(cx);
        self.refresh(false, cx);
        cx.refresh_windows();
    }

    pub(crate) fn finish_review(&mut self, cx: &mut Context<Self>) {
        if matches!(self.cleaning, Cleaning::Done(_)) {
            self.cleaning = Cleaning::Idle;
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
            self.warning = Some("No home folder was found. Use Choose folder instead.".into());
            cx.notify();
        }
    }

    fn open_selected(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(cached), Some(&id)) = (self.cached(cx), self.selection.selected.last())
            && cached.tree.node(id).kind() == EntryKind::Dir
        {
            self.navigate(cached, id, true, window, cx);
        }
    }

    /// Selects `id` from the treemap; it may be a row of the list or one level deeper.
    fn pick(&mut self, id: NodeId, clicks: usize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(table) = self.table.clone() else {
            return;
        };
        let row = table
            .read(cx)
            .delegate()
            .rows
            .iter()
            .position(|&row| row == id);
        if clicks >= 2 {
            self.selection.select_single(id);
            self.open_selected(window, cx);
            return;
        }
        if let Some(row) = row {
            table.update(cx, |table, cx| table.set_selected_row(row, cx));
        } else {
            table.update(cx, TableState::clear_selection);
            self.selection.select_single(id);
        }
        cx.notify();
    }

    fn parent(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let (Some(cached), Some(folder)) = (self.cached(cx), self.folder(cx))
            && let Some(parent) = cached.tree.parent(folder)
        {
            self.navigate(cached, parent, true, window, cx);
        }
    }

    /// What the detail card and path actions apply to: the selection, or the open folder.
    fn subject(&self, cx: &App) -> Option<NodeId> {
        self.selection
            .selected
            .last()
            .copied()
            .or_else(|| self.folder(cx))
    }

    fn selection_path(&self, cx: &App) -> Option<PathBuf> {
        Some(self.cached(cx)?.tree.path(self.subject(cx)?))
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
        if let Some(cached) = self.cached(cx) {
            panels::issues(cached, window, cx);
        }
    }

    fn stage_selection(&mut self, cx: &mut Context<Self>) {
        if self.selection.selected.is_empty() {
            return;
        }
        self.selection.stage_all();
        self.sync_staged(cx);
    }
}

impl Render for Workbench {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.fit_list(cx);
        let p = *theme::palette(cx);
        let mut body = h_flex()
            .flex_1()
            .min_h_0()
            .items_stretch()
            .child(self.sidebar(cx))
            .child(self.main(window, cx));
        if self.table.is_none() {
            body = body.bg(p.window);
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
            .bg(p.window)
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
            .on_action(cx.listener(|_, _: &OpenSettings, window, cx| panels::settings(window, cx)))
            .on_action(cx.listener(|this, _: &ShowIssues, window, cx| this.issues(window, cx)))
            .on_action(cx.listener(|this, _: &ShowSuggestions, window, cx| {
                this.show_suggestions(window, cx);
            }))
            .on_action(cx.listener(|this, _: &ToggleSelectionMode, _, cx| {
                this.selection.selected.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &TrashSelection, window, cx| {
                this.stage_selection(cx);
                panels::review(window, cx);
            }))
            .on_action(cx.listener(|this, _: &StageSelection, _, cx| this.stage_selection(cx)))
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
            .child(self.toolbar(window, cx))
            .child(body)
            .children(self.table.is_some().then(|| self.tray(cx)))
    }
}
