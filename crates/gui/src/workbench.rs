use std::path::PathBuf;
use std::time::{Duration, Instant};

use clrs_core::{Cached, EntryKind, NodeId, Progress, Source};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::table::{DataTable, TableEvent, TableState};
use gpui_kit::component::{
    ActiveTheme as _, Disableable as _, Selectable as _, Sizable as _, Size, StyledExt as _,
    h_flex, v_flex,
};
use gpui_kit::{
    AppContext as _, ClipboardItem, Context, Entity, FocusHandle, Focusable as _,
    InteractiveElement as _, IntoElement, ParentElement as _, PathPromptOptions, Render, Role,
    StatefulInteractiveElement as _, Styled as _, Subscription, Task, Window, div, px,
};

use crate::actions::{
    Back, CancelSearch, ChooseFolder, CopyPath, Forward, FreshScan, OpenFolder, OpenSettings,
    ParentFolder, Refresh, Reveal, ReviewChanges, ScanHome, ShowIssues, StageSelection, StopScan,
    ToggleInspector, ToggleSelectionMode, TrashSelection, UnstageSelection,
};
use crate::explore::Directory;
use crate::jobs::{Event, Jobs, ScanRequest};
use crate::selection::SelectionState;
use crate::settings::{Appearance, Density, Loaded, Settings};
use crate::{format, panels, theme};
use std::sync::Arc;

struct Running {
    progress: Arc<Progress>,
    started: Instant,
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
        let mut this = Self {
            settings: loaded.settings,
            jobs,
            root: None,
            running: None,
            table: None,
            table_subscription: None,
            selection: SelectionState::default(),
            inspector: true,
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
                    self.settings.remember(cached.tree.root_path());
                    self.root = Some(cached.tree.root_path().to_path_buf());
                    let directory = cached.tree.root();
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
        let table = cx.new(|cx| {
            TableState::new(Directory::new(cached, directory), window, cx)
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
                                this.selection.toggle(id);
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
            );
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
                .child(panels::detail("Kind", format::kind(node.kind()).into(), cx));
            if node.kind() == EntryKind::Dir {
                panel = panel.child(panels::detail("Files", format::count(node.files()), cx));
            }
            panel = panel.child(panels::detail("Path", format::path(&directory.cached.tree.path(id)), cx))
                .child(Button::new("reveal").label("Reveal in Finder").primary().on_click(cx.listener(|this, _, _, cx| this.reveal(cx))))
                .child(Button::new("copy-path").label("Copy Path").on_click(cx.listener(|this, _, _, cx| this.copy_path(cx))))
                .child(div().text_color(cx.theme().muted_foreground).child("Sizes use allocated disk space. Hard links count once; symlinks are not followed."));
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
                .child(div().text_color(cx.theme().muted_foreground).child("Read-only exploration. Nothing is moved or deleted.")));
        }
        content
    }
}

impl Render for Workbench {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
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
            .on_action(cx.listener(|this, _: &TrashSelection, _, cx| {
                this.selection.selected.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &StageSelection, _, cx| {
                this.selection.stage_all();
                cx.notify();
            }))
            .on_action(cx.listener(|this, _: &UnstageSelection, _, cx| {
                this.selection.selected.clear();
                cx.notify();
            }))
            .on_action(cx.listener(|_, _: &Back, _window, _cx| {
                // Placeholder - implement history later
            }))
            .on_action(cx.listener(|_, _: &Forward, _window, _cx| {
                // Placeholder - implement history later
            }))
            .on_action(cx.listener(|_, _: &CancelSearch, _window, _cx| {
                // Placeholder - implement search later
            }))
            .on_action(cx.listener(|_, _: &ReviewChanges, _window, _cx| {
                // Placeholder - implement review later
            }))
            .child(toolbar)
            .child(body)
            .child(
                div()
                    .id("scan-status")
                    .role(Role::Status)
                    .aria_label(self.status.clone())
                    .h(px(30.))
                    .px_4()
                    .py_1()
                    .border_t_1()
                    .border_color(cx.theme().border)
                    .text_color(cx.theme().muted_foreground)
                    .child(self.status.clone()),
            )
    }
}
