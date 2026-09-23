// Versioned, in-process ABI. There is NO privileged helper socket or executable RPC.
// Only the normal Rust Backend reaches this boundary after broker authorization.
import Foundation
import AppKit
import ApplicationServices
import Darwin

final class SWRequest {
    let id:String;let command:String;let args:[String:Any]
    var sideEffectsStarted=false
    init(_ raw:[String:Any]) throws {
        guard raw["version"] as? Int==1,let id=raw["id"] as? String,let command=raw["command"] as? String,let args=raw["args"] as? [String:Any] else {
            throw SWFailure(code:"ProtocolMismatch",message:"Invalid native ABI envelope")
        }
        try require(id.utf8.count==32 && command.utf8.count<=80)
        self.id=id;self.command=command;self.args=args
    }
    func checkpoint()throws{try CancellationRegistry.shared.check(id)}
    func mutation()throws{try checkpoint();sideEffectsStarted=true}
}
private final class Completion {
    let semaphore=DispatchSemaphore(value:0);private let lock=NSLock();private var data:Data?
    func finish(_ value:Data){lock.lock();data=value;lock.unlock();semaphore.signal()}
    func take()->Data?{lock.lock();defer{lock.unlock()};return data}
}
func encodeResponse(_ value:[String:Any])->Data {
    if let data=try? JSONSerialization.data(withJSONObject:value,options:[.sortedKeys]),data.count<=1_048_576{return data}
    return Data(#"{"ok":false,"error":{"code":"ResourceExhausted","message":"Native response exceeds frame budget","outcome_known":false}}"#.utf8)
}
private func copyResult(_ data:Data,_ length:UnsafeMutablePointer<Int>?)->UnsafeMutableRawPointer?{
    guard let length=length,data.count<=1_048_576,let p=malloc(max(1,data.count)) else{return nil}
    data.withUnsafeBytes{b in if let address=b.baseAddress{memcpy(p,address,b.count)}}
    length.pointee=data.count;return p
}
@_cdecl("semwright_native_begin")
public func semwrightNativeBegin(_ id:UnsafePointer<CChar>?)->Int32{
    guard let id=id else{return -1}
    return CancellationRegistry.shared.begin(String(cString:id)) ? 0 : -1
}
@_cdecl("semwright_native_cancel")
public func semwrightNativeCancel(_ id:UnsafePointer<CChar>?) {
    if let id=id{CancellationRegistry.shared.cancel(String(cString:id))}
}
@_cdecl("semwright_native_free")
public func semwrightNativeFree(_ bytes:UnsafeMutableRawPointer?){free(bytes)}
@_cdecl("semwright_native_pump")
public func semwrightNativePump(){
    guard Thread.isMainThread else{return}
    _=RunLoop.main.run(mode:.default,before:Date(timeIntervalSinceNow:0.01))
}
@_cdecl("semwright_native_call")
public func semwrightNativeCall(_ bytes:UnsafePointer<UInt8>?,_ length:Int,_ outputLength:UnsafeMutablePointer<Int>?)->UnsafeMutableRawPointer?{
    guard !Thread.isMainThread,let bytes=bytes,length>0,length<=1_048_576 else{
        return copyResult(encodeResponse(["ok":false,"error":SWFailure(code:"ProtocolMismatch",message:"Invalid native call context").json]),outputLength)
    }
    do{
        let input=Data(bytes:bytes,count:length)
        guard let raw=try JSONSerialization.jsonObject(with:input) as? [String:Any] else{throw SWFailure(code:"ProtocolMismatch",message:"Object envelope required")}
        let request=try SWRequest(raw)
        defer{CancellationRegistry.shared.finish(request.id)}
        try request.checkpoint()
        let done=Completion()
        DispatchQueue.main.async {
            Task { @MainActor in
                let result:[String:Any]
                do {try request.checkpoint();let value=try await NativeHost.shared.handle(request);result=["ok":true,"data":value]}
                catch let error as SWFailure {
                    var safe=error;safe.known=error.known && !request.sideEffectsStarted
                    result=["ok":false,"error":safe.json]
                } catch {
                    result=["ok":false,"error":SWFailure(code:"BackendFailed",message:"Native API failed; platform payload redacted",known:!request.sideEffectsStarted).json]
                }
                done.finish(encodeResponse(result))
            }
        }
        if done.semaphore.wait(timeout:.now()+32) == .timedOut {
            CancellationRegistry.shared.cancel(request.id)
            return copyResult(encodeResponse(["ok":false,"error":SWFailure(code:"Timeout",message:"Native worker exceeded deadline",known:false).json]),outputLength)
        }
        return copyResult(done.take() ?? encodeResponse(["ok":false,"error":SWFailure(code:"Internal",message:"Missing native response",known:false).json]),outputLength)
    }catch let error as SWFailure{return copyResult(encodeResponse(["ok":false,"error":error.json]),outputLength)}
    catch{return copyResult(encodeResponse(["ok":false,"error":SWFailure(code:"InvalidArgument",message:"Malformed native request").json]),outputLength)}
}

@MainActor final class NativeHost {
    static let shared=NativeHost()
    private init(){ _=NSApplication.shared.setActivationPolicy(.accessory) }
    let ledger=RefLedger<AXUIElement?>()
    var observers:[Int32:AXObserver]=[:]
    var revision:UInt64=0
    var previousTrust=false
    var artifacts:ArtifactStore?
    var captureBusy=false
    func requireAX()throws{
        guard AXIsProcessTrusted() else{
            ledger.clear();throw SWFailure(code:"PermissionDenied",message:"Accessibility permission must be granted by the user")
        }
    }
    func launch(_ pid:Int32)->Double?{NSRunningApplication(processIdentifier:pid)?.launchDate?.timeIntervalSince1970}
    func appID(_ app:NSRunningApplication)->String{bounded(app.bundleIdentifier ?? "pid:\(app.processIdentifier)",255)}
    func stamp(_ kind:String,_ app:NSRunningApplication,_ element:AXUIElement?)throws->RefStamp{
        guard let started=app.launchDate?.timeIntervalSince1970 else{throw SWFailure(code:"Unavailable",message:"Application launch identity unavailable")}
        return try ledger.insert(kind:kind,pid:app.processIdentifier,launch:started,app:appID(app),value:element,now:ProcessInfo.processInfo.systemUptime)
    }
    func resolve(_ args:[String:Any],kind:String?=nil)throws->RefRecord<AXUIElement?>{
        let s=try RefStamp.parse(args["_target"])
        let r=try ledger.resolve(s,now:ProcessInfo.processInfo.systemUptime,launch:launch)
        if let kind=kind{try require(s.kind==kind,"StaleReference","Incorrect native target kind")}
        if let e=r.value{
            try requireAX()
            var pid:pid_t=0;try axCheck(AXUIElementGetPid(e,&pid))
            try require(pid==r.pid,"StaleReference","Accessibility object process changed")
            _=try axString(e,kAXRoleAttribute,limit:80)
        }
        return r
    }
    func apps()->[NSRunningApplication]{
        Array(NSWorkspace.shared.runningApplications.filter{!$0.isTerminated && $0.processIdentifier > 0 && $0.processIdentifier != getpid() && $0.launchDate != nil}.sorted{$0.processIdentifier<$1.processIdentifier}.prefix(256))
    }
    func handle(_ r:SWRequest)async throws->[String:Any]{
        try r.checkpoint()
        switch r.command {
        case "configure":
            guard let p=r.args["artifact_directory"] as? String else{throw SWFailure(code:"InvalidArgument",message:"Artifact directory missing")}
            try require(artifacts==nil,"Conflict","Native host is already configured")
            _=NSApplication.shared
            NSApplication.shared.setActivationPolicy(.accessory)
            artifacts=try ArtifactStore(path:p)
            return ["native_abi":1,"minimum_macos":"14.0","driver_isolation":"unavailable"]
        case "permissions":return permissions()
        case "self_test":return try nativeSafeTests()
        case "app.list":
            var rows:[[String:Any]]=[]
            for app in apps(){try r.checkpoint();let s=try stamp("app",app,nil);rows.append(["ref":["$ref":s.json],"app":appID(app),"name":bounded(app.localizedName ?? "",255),"pid":app.processIdentifier,"active":app.isActive])}
            return ["apps":rows,"partial":rows.count==256]
        case "window.list":return try windows(r)
        case "ui.snapshot":return try snapshot(r)
        case "validate":_=try resolve(r.args);return["valid":true]
        case "focused":let t=try resolve(r.args,kind:"win");return["focused":try focused(t)]
        case "window.focus","window.move","window.resize","window.close":return try windowAction(r)
        case "ui.invoke","ui.toggle","ui.expand","ui.set_text","ui.read_text","ui.set_value","ui.get_value":return try uiAction(r)
        case "input.key","input.type","pointer.move","pointer.click","pointer.scroll":return try input(r)
        case "clipboard.read","clipboard.write":return try clipboard(r)
        case "screen.capture":return try await capture(r)
        case "shutdown":
            for (_,observer) in observers {CFRunLoopRemoveSource(CFRunLoopGetMain(),AXObserverGetRunLoopSource(observer),.defaultMode)}
            observers.removeAll();ledger.clear();artifacts?.clear();artifacts=nil
            return["shutdown":true]
        default:throw SWFailure(code:"Unsupported",message:"Native operation is not implemented")
        }
    }
}
