// Human-only service control. There is no model-callable register/approve endpoint.
import AppKit
import ServiceManagement

@MainActor final class Controller:NSObject,NSApplicationDelegate {
    var window:NSWindow!
    var status:NSTextField!
    let service=SMAppService.agent(plistName:"org.semwright.agent.plist")
    func applicationDidFinishLaunching(_ notification:Notification){
        let app=NSApplication.shared
        app.setActivationPolicy(.regular)
        window=NSWindow(contentRect:NSRect(x:0,y:0,width:550,height:290),styleMask:[.titled,.closable,.miniaturizable],backing:.buffered,defer:false)
        window.title="Semwright — local service"
        let heading=NSTextField(wrappingLabelWithString:"The login service starts in observe-only mode. Accessibility and capture permissions are granted separately by you in macOS. Service registration does not grant Semwright policy permissions.")
        status=NSTextField(labelWithString:"")
        let start=NSButton(title:"Register login service…",target:self,action:#selector(register))
        let stop=NSButton(title:"Unregister login service…",target:self,action:#selector(unregister))
        let settings=NSButton(title:"Open Login Items settings",target:self,action:#selector(openSettings))
        let stack=NSStackView(views:[heading,status,start,stop,settings]);stack.orientation = .vertical;stack.alignment = .leading;stack.spacing=16
        stack.translatesAutoresizingMaskIntoConstraints=false;window.contentView!.addSubview(stack)
        NSLayoutConstraint.activate([stack.leadingAnchor.constraint(equalTo:window.contentView!.leadingAnchor,constant:24),stack.trailingAnchor.constraint(equalTo:window.contentView!.trailingAnchor,constant:-24),stack.topAnchor.constraint(equalTo:window.contentView!.topAnchor,constant:24)])
        refresh();window.center();window.makeKeyAndOrderFront(nil);app.activate(ignoringOtherApps:true)
    }
    func refresh(){status.stringValue="Login service status: \(service.status.rawValue). No TCC prompt was requested."}
    func confirm(_ message:String)->Bool{
        let alert=NSAlert();alert.messageText=message;alert.informativeText="This changes only your per-user login service. No root helper, accessibility grant, or broker policy grant is created.";alert.addButton(withTitle:"Continue");alert.addButton(withTitle:"Cancel")
        return alert.runModal() == .alertFirstButtonReturn
    }
    func reportFailure(){let alert=NSAlert();alert.messageText="Service operation failed";alert.informativeText="Review the app's signature, its installed location, and Login Items settings. Platform error payloads are not recorded.";alert.runModal()}
    @objc func register(){guard confirm("Register Semwright at login?") else{return};do{try service.register()}catch{reportFailure()};refresh()}
    @objc func unregister(){guard confirm("Remove Semwright's login registration?") else{return};service.unregister(completionHandler:{ error in Task{@MainActor in if error != nil{self.reportFailure()};self.refresh()}})}
    @objc func openSettings(){SMAppService.openSystemSettingsLoginItems()}
    func applicationShouldTerminateAfterLastWindowClosed(_ sender:NSApplication)->Bool{true}
}
@main struct ControlMain {
    @MainActor static func main(){let app=NSApplication.shared;let owner=Controller();app.delegate=owner;withExtendedLifetime(owner){app.run()}}
}
