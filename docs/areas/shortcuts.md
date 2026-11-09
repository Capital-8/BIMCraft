# Keyboard shortcuts

Two kinds of bindings live side by side:

- **Two-character sequences**, as in Revit (`WA`, `DR`, `KF`). Their list is `SHORTCUTS` in `lib.rs`; Manage > Shortcuts
  (`shortcuts_window` in `shortcuts.rs`) adds, changes and loads the user's own. The user's bindings live in
  `settings.json` (the `shortcuts` field: a two-character key to the English name of an action) and are read into
  `BimApp::custom_shortcuts` at startup (`use_settings`). The dialog binds sequences only: it cannot yet give an action a
  chord with a modifier (`Ctrl+…`) or a function key (`F…`).
- **Fixed chords** for the window's frequent commands: files, undo, find, the start page and the alignment guides.
  They are checked in `BimApp::keys` (`lib.rs`) and cannot be changed.

## Home and the alignment guides

Revit opens its Home page with `Ctrl+D`. BIMCraft's start page (Home) moved off that chord to `Ctrl+U`, and `F11` stopped
toggling the alignment guides, which `F8` still does:

| Key or action | Before | Now |
|---|---|---|
| Home, the start page | `Ctrl+D` | `Ctrl+U` (`⌘U` on macOS) |
| `Ctrl+D` | Home | nothing |
| Alignment guides | `F8` and `F11` | `F8` only |
| `F11` | the alignment guides | nothing: left to the system, which may use it for full screen |
| `Ctrl+U` in the note editor | Underline | Underline, unchanged |
| `Ctrl+Г` on the Russian layout | nothing | Home, as egui reports the physical `U` key |

Why: Home is gone to rarely, so `Ctrl+U` does for it, and `Ctrl+D`, the easiest chord for one hand, is left free for a
command used more often. No command takes it yet, and the user cannot bind it until the Shortcuts dialog takes chords.
`F11` often belongs to the window manager, so the window no longer takes it.

## Context and priority

- A modal dialog takes the keys first: while one is open, `Ctrl+U` and `F8` do nothing in the window behind it.
- In a text field the keys belong to the field: `Ctrl+U` does not open Home.
- While a note is edited with Override Formatting, `Ctrl+U` stays Underline (`notes.rs`) and Home does not open.
- On an ordinary canvas `Ctrl+U` opens the start page, `Ctrl+D` does nothing, and `F8` turns the alignment guides on and
  off.
- The two-character sequences `AG` and `OG` toggle the guides too; `F8` is the only function key that does.

## Tooltips and translations

Home on the Quick Access Toolbar says `Home (Ctrl+U)` (`Home (⌘U)` on macOS), and Back on the start page says
`Back to the project (Ctrl+U)`. Both are built with `tf` from the templates `Home ({}U)` and `Back to the project ({}U)`,
which `translations/ui/bimcraft-ui.pot` lists, with the modifier filled in: `Ctrl+`, or `⌘` on macOS. The tooltips name
the key by its Latin letter. On a layout whose letters are not Latin the same physical key has another letter: on the
Russian layout it is `Г`, and `Ctrl+Г` opens Home, because egui falls back to the physical key when the layout's letter
is not one it names.
