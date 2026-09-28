# UI Automation semantic mapping

Raw UIA ControlType mapping is owned by `platform-windows/src/roles.rs`; raw AX role mapping moves to `platform-macos/src/roles.rs`. `platform-api` retains only normalized semantic concepts.

Mappings include Button/SplitButton -> button; CheckBox -> check_box; RadioButton -> radio_button; Edit/Document -> text; Text -> label; Hyperlink -> link; Menu/MenuBar/MenuItem; List/ListItem; ComboBox; Tree/TreeItem; Table/DataGrid/DataItem/Header; Slider; Spinner; Tab/TabItem; ToolBar; ScrollBar; Window; Pane; Group; ProgressBar; Image.

Implemented UIA pattern operations in source: InvokePattern (`ui.invoke`), ValuePattern (`ui.set_text`, `ui.read_text`, value fallback), RangeValuePattern (`ui.set_value/get_value`), TogglePattern (`ui.toggle`), SelectionItemPattern (`ui.select`) and ExpandCollapsePattern (`ui.expand`). Unsupported patterns return Unsupported rather than synthesizing blind clicks.

The snapshot exposes names/AutomationId/framework/class/enabled/focused/password/bounds and children under hard budgets. UI strings are untrusted data; they are returned as data only and cannot grant authority. The Windows host now installs bounded native UIA focus/property/structure/text/selection subscriptions, projects them into the portable event taxonomy and advances structural generations conservatively. Hosted tests cover the contract, but native event fidelity under real application churn remains `WINDOWS_INTERACTIVE_PENDING`.
