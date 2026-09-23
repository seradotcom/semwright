import Foundation
import AppKit
import CoreGraphics
import ApplicationServices
import Security
import ServiceManagement

@MainActor extension NativeHost {
    func permissions()->[String:Any]{
        let ax=AXIsProcessTrusted()
        if previousTrust && !ax{ledger.clear()}
        previousTrust=ax
        return["platform":"macos","os_version":ProcessInfo.processInfo.operatingSystemVersionString,
               "accessibility":ax,"post_events":CGPreflightPostEventAccess(),"screen_recording":CGPreflightScreenCaptureAccess(),
               "capture_picker":true,"input_monitoring_requested":false,"automation_requested":false,
               "driver_isolation":"unavailable","plugin_isolation":"unavailable",
               "tcc_owner":"main Semwright broker process; native code is in-process",
               "login_service_status":SMAppService.agent(plistName:"org.semwright.agent.plist").status.rawValue]
    }
    func clipboard(_ r:SWRequest)throws->[String:Any]{
        try r.checkpoint()
        let board=NSPasteboard.general
        if r.command=="clipboard.read"{
            let limit=Int(try number(r.args["max_bytes"],default:65536,range:1...1048576))
            // NSPasteboard has no length-before-read API; reject the returned data
            // before conversion/transport. Lazy external providers may be unresponsive.
            guard let data=board.data(forType:.string) else{return["text":"","bytes":0]}
            try require(data.count<=limit,"ResourceExhausted","Clipboard exceeds byte budget")
            guard let text=String(data:data,encoding:.utf8) else{throw SWFailure(code:"Unsupported",message:"Clipboard text is not UTF-8")}
            return["text":text,"bytes":data.count]
        }
        guard let text=r.args["text"] as? String else{throw SWFailure(code:"InvalidArgument",message:"Text required")}
        try require(text.utf8.count<=65536)
        try r.mutation();board.clearContents()
        try require(board.setString(text,forType:.string),"BackendFailed","Clipboard write failed")
        return["written":true,"bytes":text.utf8.count]
    }
    func nativeSafeTests()throws->[String:Any]{
        let board=NSPasteboard.withUniqueName();defer{board.releaseGlobally()}
        try require(board.setString("Semwright fixture ✓",forType:.string),"BackendFailed","Unique pasteboard write failed")
        try require(board.string(forType:.string)=="Semwright fixture ✓","BackendFailed","Unique pasteboard round trip failed")
        let count=NSWorkspace.shared.runningApplications.count
        return["unique_pasteboard":true,"workspace_enumeration":true,"application_count":count,"tcc_prompted":false,"general_clipboard_touched":false]
    }
}
struct SignatureValidator {
    /// Owner-configured requirements only. Not exposed through public command args.
    /// This is signature evidence, NOT execution authorization or sandbox enforcement.
    static func validate(_ url:URL,requirement:String)throws {
        try require(url.isFileURL && requirement.utf8.count<=4096)
        var code:SecStaticCode?,rule:SecRequirement?
        try require(SecStaticCodeCreateWithPath(url as CFURL,SecCSFlags(),&code)==errSecSuccess,"PermissionDenied","Static code identity unavailable")
        try require(SecRequirementCreateWithString(requirement as CFString,SecCSFlags(),&rule)==errSecSuccess,"InvalidArgument","Invalid owner signature requirement")
        guard let code=code,let rule=rule else{throw SWFailure(code:"PermissionDenied",message:"Signature identity unavailable")}
        let flags=SecCSFlags(rawValue:kSecCSCheckAllArchitectures)
        try require(SecStaticCodeCheckValidity(code,flags,rule)==errSecSuccess,"PermissionDenied","Signature requirement was not satisfied")
    }
}
