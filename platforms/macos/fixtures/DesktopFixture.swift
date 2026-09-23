import AppKit
@MainActor final class Fixture:NSObject,NSApplicationDelegate {
    var window:NSWindow!;var result:NSTextField!;var counter=0
    func applicationDidFinishLaunching(_ notification:Notification){
        NSApp.setActivationPolicy(.regular)
        window=NSWindow(contentRect:NSRect(x:80,y:80,width:600,height:400),styleMask:[.titled,.closable,.resizable],backing:.buffered,defer:false)
        window.title="Semwright owned accessibility fixture"
        let text=NSTextField(string:"Synthetic editable text ✓");text.setAccessibilityIdentifier("fixture.text")
        let secure=NSSecureTextField(string:"fixture-only-password");secure.setAccessibilityIdentifier("fixture.secure")
        let button=NSButton(title:"Apply",target:self,action:#selector(apply));button.setAccessibilityIdentifier("fixture.apply")
        let duplicate=NSButton(title:"Apply",target:self,action:#selector(apply));duplicate.setAccessibilityIdentifier("fixture.duplicate")
        let toggle=NSButton(checkboxWithTitle:"Synthetic enabled flag",target:nil,action:nil);toggle.setAccessibilityIdentifier("fixture.toggle")
        let slider=NSSlider(value:25,minValue:0,maxValue:100,target:nil,action:nil);slider.setAccessibilityIdentifier("fixture.slider")
        result=NSTextField(labelWithString:"Applied: 0");result.setAccessibilityIdentifier("fixture.result")
        let stack=NSStackView(views:[NSTextField(labelWithString:"Synthetic fixture only — no user documents"),text,secure,button,duplicate,toggle,slider,result]);stack.orientation = .vertical;stack.spacing=16;stack.alignment = .leading
        stack.translatesAutoresizingMaskIntoConstraints=false;window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo:window.contentView!.leadingAnchor,constant:20),stack.trailingAnchor.constraint(equalTo:window.contentView!.trailingAnchor,constant:-20),stack.topAnchor.constraint(equalTo:window.contentView!.topAnchor,constant:20)])
        window.makeKeyAndOrderFront(nil);NSApp.activate(ignoringOtherApps:true)
    }
    @objc func apply(){counter+=1;result.stringValue="Applied: \(counter)"}
    func applicationShouldTerminateAfterLastWindowClosed(_ sender:NSApplication)->Bool{true}
}
@main struct FixtureMain{
    @MainActor static func main(){let app=NSApplication.shared;let fixture=Fixture();app.delegate=fixture;withExtendedLifetime(fixture){app.run()}}
}
