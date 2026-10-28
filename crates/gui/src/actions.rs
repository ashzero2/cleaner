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
        ShowIssues
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
            MenuItem::action("Enclosing Folder", ParentFolder),
            MenuItem::action("Open Folder", OpenFolder),
            MenuItem::action("Toggle Inspector", ToggleInspector),
            MenuItem::action("Scan Issues", ShowIssues),
        ]),
        Menu::new("Selection").items([
            MenuItem::action("Reveal in Finder", Reveal),
            MenuItem::action("Copy Path", CopyPath),
        ]),
    ]);
}
