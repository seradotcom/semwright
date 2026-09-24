use uiautomation::controls::ControlType;

/// Normalize UIA-native control types at the Windows boundary.
/// The portable API never sees UIA numeric IDs or `ControlType` values.
pub fn semantic_role(control: ControlType) -> &'static str {
    match control {
        ControlType::Button | ControlType::SplitButton => "button",
        ControlType::CheckBox => "check_box",
        ControlType::RadioButton => "radio_button",
        ControlType::Edit | ControlType::Document => "text",
        ControlType::Text => "label",
        ControlType::Hyperlink => "link",
        ControlType::Menu | ControlType::MenuBar => "menu",
        ControlType::MenuItem => "menu_item",
        ControlType::List => "list",
        ControlType::ListItem => "list_item",
        ControlType::ComboBox => "combo_box",
        ControlType::Tree => "tree",
        ControlType::TreeItem => "tree_item",
        ControlType::Table | ControlType::DataGrid => "table",
        ControlType::DataItem => "table_row",
        ControlType::Header | ControlType::HeaderItem => "table_header",
        ControlType::Slider => "slider",
        ControlType::Spinner => "spinner",
        ControlType::Tab => "tab_list",
        ControlType::TabItem => "tab",
        ControlType::ToolBar => "tool_bar",
        ControlType::ScrollBar => "scroll_bar",
        ControlType::Window => "window",
        ControlType::Pane => "pane",
        ControlType::Group => "group",
        ControlType::ProgressBar => "progress_bar",
        ControlType::Image => "image",
        _ => "group",
    }
}
