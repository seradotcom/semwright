// Native integration smoke, intentionally not Accessibility/TCC acceptance.
// Tests a UNIQUE pasteboard, workspace enumeration, and kernel peer credentials.
import Foundation
import Darwin
import CSemwrightNative
final class SharedResult:@unchecked Sendable{
    let lock=NSLock();var result:Int32?
    func set(_ n:Int32){lock.lock();result=n;lock.unlock()}
    func get()->Int32?{lock.lock();defer{lock.unlock()};return result}
}
@main struct Smoke {
    static func main(){
        var sockets:[Int32]=[-1,-1]
        guard socketpair(AF_UNIX,SOCK_STREAM,0,&sockets)==0 else{exit(1)}
        defer{close(sockets[0]);close(sockets[1])}
        var uid:uid_t=0,gid:gid_t=0
        guard getpeereid(sockets[0],&uid,&gid)==0,uid==getuid() else{exit(1)}
        let done=SharedResult()
        DispatchQueue.global().async{
            do{
                let id=UUID().uuidString.replacingOccurrences(of:"-",with:"").lowercased()
                guard id.withCString({semwright_native_begin($0)})==0 else{throw NSError(domain:"begin",code:1)}
                let bytes=try JSONSerialization.data(withJSONObject:["version":1,"id":id,"command":"self_test","args":[:]])
                var count=0
                let ptr=bytes.withUnsafeBytes{raw in semwright_native_call(raw.baseAddress!.assumingMemoryBound(to:UInt8.self),raw.count,&count)}
                guard let ptr=ptr else{throw NSError(domain:"call",code:1)}
                defer{semwright_native_free(ptr)}
                guard count>0,count<=1_048_576 else{throw NSError(domain:"size",code:1)}
                let result=try JSONSerialization.jsonObject(with:Data(bytes:ptr,count:count)) as? [String:Any]
                guard result?["ok"] as? Bool==true,let data=result?["data"] as? [String:Any],data["unique_pasteboard"] as? Bool==true,data["workspace_enumeration"] as? Bool==true,data["general_clipboard_touched"] as? Bool==false else{throw NSError(domain:"result",code:1)}
                print("{\"native_smoke\":\"PASS\",\"unique_pasteboard\":true,\"workspace\":true,\"getpeereid\":true,\"live_ax\":\"NOT_RUN\",\"capture\":\"NOT_RUN\"}")
                done.set(0)
            }catch{fputs("Native smoke failed; platform payload redacted\n",stderr);done.set(1)}
        }
        let deadline=ProcessInfo.processInfo.systemUptime+40
        while done.get()==nil && ProcessInfo.processInfo.systemUptime<deadline{semwright_native_pump()}
        exit(done.get() ?? 2)
    }
}
