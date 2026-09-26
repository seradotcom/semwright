// Foundation-only production model. This exact file is tested on Linux with Swift.
import Foundation

struct SWFailure: Error {
    let code: String
    let message: String
    var known: Bool = true
    var json: [String:Any] { ["code":code,"message":message,"outcome_known":known] }
}
func require(_ condition:Bool,_ code:String="InvalidArgument",_ message:String="Invalid bounded native request") throws {
    if !condition {throw SWFailure(code:code,message:message)}
}
func bounded(_ text:String,_ bytes:Int) -> String {
    var output=""; var size=0
    for scalar in text.unicodeScalars {
        let s=String(scalar); if size+s.utf8.count>bytes {break}
        output.append(s); size+=s.utf8.count
    }
    return output
}
func number(_ value:Any?,default fallback:Double,range:ClosedRange<Double>) throws -> Double {
    guard let value=value else{return fallback}
    guard let n=value as? NSNumber, CFGetTypeID(n) != CFBooleanGetTypeID() else {throw SWFailure(code:"InvalidArgument",message:"Finite number required")}
    let v=n.doubleValue; try require(v.isFinite && range.contains(v)); return v
}
import CoreFoundation
struct SWRect: Equatable {
    var x:Double; var y:Double; var width:Double; var height:Double
    func validated() throws -> SWRect {
        try require([x,y,width,height].allSatisfy{$0.isFinite} && width>0 && height>0)
        return self
    }
    func contains(_ x:Double,_ y:Double)->Bool{x>=self.x && y>=self.y && x<self.x+width && y<self.y+height}
    func pixels(in display:SWRect,scale:Double)throws->[Int] {
        _=try validated();_=try display.validated();try require(scale.isFinite && (0.5...8).contains(scale))
        let x0=max(x,display.x),y0=max(y,display.y),x1=min(x+width,display.x+display.width),y1=min(y+height,display.y+display.height)
        try require(x1>x0 && y1>y0)
        let values=[floor((x0-display.x)*scale),floor((y0-display.y)*scale),ceil((x1-display.x)*scale),ceil((y1-display.y)*scale)]
        try require(values.allSatisfy{$0.isFinite && abs($0)<1e9})
        let a=values.map{Int($0)};return[a[0],a[1],a[2]-a[0],a[3]-a[1]]
    }
    var json:[String:Any]{["x":Int(floor(x)),"y":Int(floor(y)),"width":Int(ceil(width)),"height":Int(ceil(height)),"coordinate_space":"quartz_global_points"]}
}
enum Semantic {
    static let roles:[String:String]=[
        "AXApplication":"application","AXWindow":"window","AXButton":"button",
        "AXCheckBox":"check_box","AXRadioButton":"radio_button","AXTextField":"text",
        "AXTextArea":"text","AXStaticText":"label","AXMenu":"menu","AXMenuItem":"menu_item",
        "AXTable":"table","AXRow":"table_row","AXCell":"table_cell","AXSlider":"slider",
        "AXPopUpButton":"combo_box","AXToolbar":"tool_bar","AXScrollArea":"scroll_pane"
    ]
    static func role(_ raw:String)->String{roles[raw] ?? "group"}
    static func secure(_ role:String,_ subrole:String)->Bool{role=="AXSecureTextField" || subrole=="AXSecureTextField" || subrole=="AXSecureTextEntryArea"}
    static func states(enabled:Bool,focused:Bool,selected:Bool,expanded:Bool)->[String]{
        [(enabled,"enabled"),(focused,"focused"),(selected,"selected"),(expanded,"expanded")].filter{$0.0}.map{$0.1}
    }
    static func unicodeChunks(_ text:String,maxBytes:Int=4096)throws->[[UInt16]]{
        try require(text.utf8.count<=maxBytes)
        // Validate ALL clusters before the first side effect. Never split a surrogate pair.
        var chunks:[[UInt16]]=[];var chunk:[UInt16]=[]
        for character in text {
            let units=Array(String(character).utf16)
            try require(units.count<=20,"Unsupported","Unicode cluster exceeds native input chunk bound")
            if chunk.count+units.count>20 {chunks.append(chunk);chunk=[]}
            chunk.append(contentsOf:units)
        }
        if !chunk.isEmpty{chunks.append(chunk)};return chunks
    }
}
struct RefStamp:Equatable {
    let kind:String;let identity:String;let revision:UInt64;let fingerprint:String;let app:String
    var json:[String:Any]{["kind":kind,"identity":identity,"revision":revision,"fingerprint":fingerprint,"app":app]}
    static func parse(_ v:Any?) throws -> RefStamp {
        guard let d=v as? [String:Any], let kind=d["kind"] as? String,let id=d["identity"] as? String,
              let n=d["revision"] as? NSNumber,CFGetTypeID(n) != CFBooleanGetTypeID(),let f=d["fingerprint"] as? String,let app=d["app"] as? String else {throw SWFailure(code:"StaleReference",message:"Broker-resolved native target required")}
        try require(["app","win","ui","screen","process"].contains(kind) && id.utf8.count<=80 && f.utf8.count<=128 && app.utf8.count<=255 && n.doubleValue.isFinite && n.doubleValue>=0 && n.doubleValue<18446744073709551616.0 && n.doubleValue.rounded(.towardZero)==n.doubleValue,"StaleReference","Invalid native ref stamp")
        return RefStamp(kind:kind,identity:id,revision:n.uint64Value,fingerprint:f,app:app)
    }
}
final class SemanticEventEpoch {
    static let shared=SemanticEventEpoch()
    private let lock=NSLock()
    private var value:UInt64=1
    init(){}
    func current()->UInt64{lock.lock();defer{lock.unlock()};return value}
    @discardableResult func bump()->UInt64{
        lock.lock();defer{lock.unlock()}
        value = value == UInt64.max ? 1 : value + 1
        return value
    }
}
struct RefRecord<T>{let stamp:RefStamp;let pid:Int32;let launch:Double;let expires:Double;let eventEpoch:UInt64;let value:T}
final class RefLedger<T>{
    private(set) var entries:[String:RefRecord<T>]=[:]
    private var generations:[Int32:UInt64]=[:]
    private let eventEpoch:SemanticEventEpoch
    let capacity:Int
    init(capacity:Int=8192,eventEpoch:SemanticEventEpoch = .shared){self.capacity=capacity;self.eventEpoch=eventEpoch}
    func generation(_ pid:Int32)->UInt64{generations[pid] ?? 1}
    func invalidate(_ pid:Int32){
        generations[pid]=generation(pid) &+ 1
        entries=entries.filter{$0.value.pid != pid}
    }
    func prune(_ now:Double){entries=entries.filter{$0.value.expires>now}}
    func insert(kind:String,pid:Int32,launch:Double,app:String,value:T,now:Double)throws->RefStamp{
        prune(now);try require(entries.count<capacity,"ResourceExhausted","Native reference capacity exhausted")
        try require(launch.isFinite && now.isFinite && now>=0)
        let stamp=RefStamp(kind:kind,identity:UUID().uuidString.lowercased(),revision:generation(pid),fingerprint:UUID().uuidString.lowercased(),app:app)
        entries[stamp.identity]=RefRecord(stamp:stamp,pid:pid,launch:launch,expires:now+60,eventEpoch:eventEpoch.current(),value:value)
        return stamp
    }
    func resolve(_ s:RefStamp,now:Double,launch:(Int32)->Double?)throws->RefRecord<T>{
        guard let e=entries[s.identity],e.stamp==s,e.expires>now,e.stamp.revision==generation(e.pid),e.eventEpoch==eventEpoch.current(),launch(e.pid)==e.launch else {
            throw SWFailure(code:"StaleReference",message:"Native identity expired, changed, or was destroyed")
        }
        return e
    }
    func clear(){entries.removeAll();generations.removeAll()}
}
final class CancellationRegistry {
    static let shared=CancellationRegistry()
    private let lock=NSLock()
    private var requests:[String:(cancelled:Bool,deadline:Double)]=[:]
    func begin(_ id:String,now:Double=ProcessInfo.processInfo.systemUptime)->Bool{
        lock.lock();defer{lock.unlock()}
        requests=requests.filter{$0.value.deadline>now}
        guard id.utf8.count==32,id.allSatisfy({$0.isHexDigit}),requests.count<16,requests[id]==nil else{return false}
        requests[id]=(false,now+30);return true
    }
    func cancel(_ id:String){lock.lock();defer{lock.unlock()};if let old=requests[id]{requests[id]=(true,old.deadline)}}
    func finish(_ id:String){lock.lock();defer{lock.unlock()};requests.removeValue(forKey:id)}
    func check(_ id:String,now:Double=ProcessInfo.processInfo.systemUptime)throws{
        lock.lock();defer{lock.unlock()}
        guard let state=requests[id],!state.cancelled else {throw SWFailure(code:"Cancelled",message:"Native operation cancelled")}
        try require(now<state.deadline,"Timeout","Native deadline elapsed")
    }
}
