/// Raw Accessibility role normalization belongs to the macOS backend, not platform-api.
pub fn ax_role(role: &str) -> &'static str {
    match role {
        "AXApplication" => "application",
        "AXWindow" | "AXSheet" | "AXDialog" => "window",
        "AXButton" => "button",
        "AXCheckBox" => "check_box",
        "AXRadioButton" => "radio_button",
        "AXTextField" | "AXTextArea" => "text",
        "AXStaticText" => "label",
        "AXLink" => "link",
        "AXMenu" | "AXMenuBar" => "menu",
        "AXMenuItem" => "menu_item",
        "AXList" => "list",
        "AXRow" => "list_item",
        "AXPopUpButton" | "AXComboBox" => "combo_box",
        "AXOutline" => "tree",
        "AXTable" => "table",
        "AXSlider" => "slider",
        "AXTabGroup" => "tab_list",
        "AXToolbar" => "tool_bar",
        "AXScrollBar" => "scroll_bar",
        "AXScrollArea" => "scroll_pane",
        "AXImage" => "image",
        "AXGroup" | "AXSplitGroup" => "group",
        _ => "group",
    }
}
