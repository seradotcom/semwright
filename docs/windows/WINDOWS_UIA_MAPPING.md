# UI Automation semantic mapping

Raw UIA ControlType mapping is owned by `platform-windows/src/roles.rs`; raw AX role mapping moves to `platform-macos/src/roles.rs`. `platform-api` retains only normalized semantic concepts.

Mappings include Button/SplitButton -> button; CheckBox -> check_box; RadioButton -> radio_button; Edit/Document -> text; Text -> label; Hyperlink -> link; Menu/MenuBar/MenuItem; List/ListItem; ComboBox; Tree/TreeItem; Table/DataGrid/DataItem/Header; Slider; Spinner; Tab/TabItem; ToolBar; ScrollBar; Window; Pane; Group; ProgressBar; Image.

Implemented UIA pattern operations in source: InvokePattern (`ui.invoke`), ValuePattern (`ui.set_text`, `ui.read_text`, value fallback), RangeValuePattern (`ui.set_value/get_value`), TogglePattern (`ui.toggle`), SelectionItemPattern (`ui.select`) and ExpandCollapsePattern (`ui.expand`). Unsupported patterns return Unsupported rather than synthesizing blind clicks.

The snapshot exposes names/AutomationId/framework/class/enabled/focused/password/bounds and children under hard budgets. UI strings are untrusted data; they are returned as data only and cannot grant authority. External UIA event subscription is not yet enabled; the included bounded event queue is ready for invalidation semantics, but `WINDOWS_INTERACTIVE_PENDING` applies to event fidelity.
