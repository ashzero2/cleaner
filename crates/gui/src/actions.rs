use gpui_kit::{App, KeyBinding, Menu, MenuItem};

gpui::actions!(
    clrs,
    [
        Quit,
        ChooseFolder,
        ScanHome,
        Refresh,
        FreshScan,
        StopScan,
        ParentFolder,
        OpenFolder,
        Reveal,
        CopyPath,
        ToggleInspector,
        OpenSettings,
        ShowIssues,
        ToggleSelectionMode,
        TrashSelection,
        StageSelection,
        UnstageSelection,
        ReviewChanges,
        CancelSearch,
        OpenLargestFiles,
        Back,
        Forward,
        Find,
        ClearStaged
    ]
);

pub(crate) fn init(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new("cmd-q", Quit, None),
        KeyBinding::new("cmd-o", ChooseFolder, Some("Workbench")),
        KeyBinding::new("cmd-r", Refresh, Some("Workbench")),
        KeyBinding::new("cmd-shift-r", FreshScan, Some("Workbench")),
        KeyBinding::new("cmd-.", StopScan, Some("Workbench")),
        KeyBinding::new("cmd-up", ParentFolder, Some("Workbench")),
        KeyBinding::new("cmd-down", OpenFolder, Some("Workbench")),
        KeyBinding::new("cmd-i", ToggleInspector, Some("Workbench")),
        KeyBinding::new("cmd-shift-c", CopyPath, Some("Workbench")),
        KeyBinding::new("cmd-,", OpenSettings, Some("Workbench")),
        KeyBinding::new("escape", CancelSearch, Some("Workbench")),
        KeyBinding::new("cmd-\\", TrashSelection, Some("Workbench")),
        KeyBinding::new("cmd-t", StageSelection, Some("Workbench")),
        KeyBinding::new("cmd-shift-t", UnstageSelection, Some("Workbench")),
        KeyBinding::new("cmd-shift-e", ReviewChanges, Some("Workbench")),
        KeyBinding::new("cmd-f", Find, Some("Workbench")),
        KeyBinding::new("cmd-l", OpenLargestFiles, Some("Workbench")),
        KeyBinding::new("cmd-[", Back, Some("Workbench")),
        KeyBinding::new("cmd-]", Forward, Some("Workbench")),
    ]);
    cx.on_action(|_: &Quit, cx| cx.quit());
    cx.set_menus([
        Menu::new("cl-rs").items([
            MenuItem::action("Settings…", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit cl-rs", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("Choose Folder…", ChooseFolder),
            MenuItem::action("Scan Home", ScanHome),
            MenuItem::separator(),
            MenuItem::action("Refresh Scan", Refresh),
            MenuItem::action("Fresh Scan", FreshScan),
            MenuItem::action("Stop Scan", StopScan),
        ]),
        Menu::new("View").items([
            MenuItem::action("Back", Back),
            MenuItem::action("Forward", Forward),
            MenuItem::action("Enclosing Folder", ParentFolder),
            MenuItem::action("Find…", Find),
            MenuItem::action("Largest Files", OpenLargestFiles),
            MenuItem::action("Open Folder", OpenFolder),
            MenuItem::action("Toggle Inspector", ToggleInspector),
            MenuItem::action("Scan Issues", ShowIssues),
        ]),
        Menu::new("Selection").items([
            MenuItem::action("Reveal in Finder", Reveal),
            MenuItem::action("Copy Path", CopyPath),
            MenuItem::separator(),
            MenuItem::action("Stage for Cleanup", StageSelection),
            MenuItem::action("Unstage", UnstageSelection),
            MenuItem::action("Clear Staged", ClearStaged),
            MenuItem::action("Review Staged…", ReviewChanges),
        ]),
    ]);
}
