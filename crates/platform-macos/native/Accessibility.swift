import Foundation
import AppKit
import ApplicationServices

func axCheck(_ result:AXError)throws{
    switch result {
    case .success:return
    case .invalidUIElement:throw SWFailure(code:"StaleReference",message:"Accessibility object was destroyed")
    case .apiDisabled:throw SWFailure(code:"PermissionDenied",message:"Accessibility permission unavailable")
    case .cannotComplete:throw SWFailure(code:"Timeout",message:"Accessibility application did not respond")
    case .attributeUnsupported,.actionUnsupported,.parameterizedAttributeUnsupported,.notImplemented,.noValue:
        throw SWFailure(code:"Unsupported",message:"Application does not implement this semantic operation")
    default:throw SWFailure(code:"BackendFailed",message:"Accessibility operation failed")
    }
}
func axRaw(_ e:AXUIElement,_ name:String)throws->CFTypeRef{
    AXUIElementSetMessagingTimeout(e,0.15)
    var value:CFTypeRef?;try axCheck(AXUIElementCopyAttributeValue(e,name as CFString,&value))
    guard let value=value else{throw SWFailure(code:"Unavailable",message:"Accessibility value absent")};return value
}
func axString(_ e:AXUIElement,_ name:String,limit:Int)throws->String{
    let v=try axRaw(e,name)
    try require(CFGetTypeID(v)==CFStringGetTypeID(),"Unsupported","Accessibility attribute is not text")
    return bounded(v as! String,limit)
}
func axBool(_ e:AXUIElement,_ name:String)->Bool{guard let value=try? axRaw(e,name) else{return false};return (value as? NSNumber)?.boolValue ?? false}
func axElement(_ value:CFTypeRef)throws->AXUIElement{
    try require(CFGetTypeID(value)==AXUIElementGetTypeID(),"Unsupported","Accessibility object expected")
    return value as! AXUIElement
}
func axElements(_ e:AXUIElement,_ attr:String,limit:Int)throws->([AXUIElement],Int){
    AXUIElementSetMessagingTimeout(e,0.15)
    var count:CFIndex=0;try axCheck(AXUIElementGetAttributeValueCount(e,attr as CFString,&count))
    try require(count>=0,"BackendFailed","Invalid accessibility collection size")
    if count==0{return([],0)}
    var array:CFArray?
    try axCheck(AXUIElementCopyAttributeValues(e,attr as CFString,0,min(count,limit),&array))
    guard let values=array as? [AnyObject] else{throw SWFailure(code:"BackendFailed",message:"Invalid accessibility collection")}
    try require(values.count<=limit,"ResourceExhausted","Accessibility collection exceeds budget")
    return(try values.map{try axElement($0)},count)
}
func axSet(_ e:AXUIElement,_ attr:String,_ value:CFTypeRef)throws{
    var settable:DarwinBoolean=false;try axCheck(AXUIElementIsAttributeSettable(e,attr as CFString,&settable))
    try require(settable.boolValue,"Unsupported","Accessibility attribute is not settable")
    try axCheck(AXUIElementSetAttributeValue(e,attr as CFString,value))
}
func axBounds(_ e:AXUIElement)throws->SWRect{
    let p=try axRaw(e,kAXPositionAttribute),s=try axRaw(e,kAXSizeAttribute)
    try require(CFGetTypeID(p)==AXValueGetTypeID() && CFGetTypeID(s)==AXValueGetTypeID(),"Unsupported","Accessibility geometry unavailable")
    let pv=p as! AXValue,sv=s as! AXValue
    try require(AXValueGetType(pv) == .cgPoint && AXValueGetType(sv) == .cgSize,"Unsupported","Invalid accessibility geometry types")
    var point=CGPoint.zero,size=CGSize.zero
    try require(AXValueGetValue(pv,.cgPoint,&point) && AXValueGetValue(sv,.cgSize,&size),"BackendFailed","Cannot read accessibility geometry")
    let rect=SWRect(x:point.x,y:point.y,width:size.width,height:size.height)
    _=try rect.validated();try require([rect.x,rect.y,rect.width,rect.height].allSatisfy{abs($0)<1e7},"ResourceExhausted","Geometry budget exceeded")
    return rect
}
func axSecure(_ e:AXUIElement)->Bool{
    guard let role=try? axString(e,kAXRoleAttribute,limit:80) else{return true}
    let sub=(try? axString(e,kAXSubroleAttribute,limit:80)) ?? ""
    return Semantic.secure(role,sub)
}
func axOptionalNumber(_ e:AXUIElement,_ key:String)->Double?{
    guard let raw=try? axRaw(e,key),let n=raw as? NSNumber,
          CFGetTypeID(n) != CFBooleanGetTypeID(),n.doubleValue.isFinite else{return nil}
    return n.doubleValue
}
func axArrayCount(_ e:AXUIElement,_ key:String)->Int?{
    guard let raw=try? axRaw(e,key),CFGetTypeID(raw)==CFArrayGetTypeID() else{return nil}
    return (raw as! NSArray).count
}
func axSettable(_ e:AXUIElement,_ key:String)->Bool?{
    var settable:DarwinBoolean=false
    guard AXUIElementIsAttributeSettable(e,key as CFString,&settable) == .success else{return nil}
    return settable.boolValue
}
private func semanticAXEvent(_ notification:CFString)->(String,Bool){
    switch notification as String {
    case kAXWindowCreatedNotification,kAXUIElementDestroyedNotification:
        return ("semantic.structure.changed",true)
    case kAXFocusedWindowChangedNotification:
        return ("semantic.focus.changed",false)
    case kAXSelectedChildrenChangedNotification:
        return ("semantic.selection.changed",false)
    case kAXValueChangedNotification,kAXTitleChangedNotification:
        return ("semantic.property.changed",false)
    default:
        return ("semantic.object.changed",false)
    }
}
private let observe:AXObserverCallback={_,element,notification,_ in
    var pid:pid_t=0
    guard AXUIElementGetPid(element,&pid) == .success else{return}
    let event=semanticAXEvent(notification)
    SemanticEventQueue.shared.push(
        kind:event.0,
        pid:pid,
        notification:notification as String,
        structural:event.1
    )
    // No native object crosses threads. Retained observers live on the main run loop.
    Task { @MainActor in NativeHost.shared.ledger.invalidate(pid) }
}
@MainActor extension NativeHost {
    func observeApp(_ app:NSRunningApplication){
        let pid=app.processIdentifier
        guard observers[pid]==nil,observers.count<128 else{return}
        var observer:AXObserver?
        guard AXObserverCreate(pid,observe,&observer) == .success,let observer=observer else{return}
        let root=AXUIElementCreateApplication(pid)
        for event in [kAXWindowCreatedNotification,kAXFocusedWindowChangedNotification,kAXUIElementDestroyedNotification,kAXValueChangedNotification,kAXTitleChangedNotification,kAXSelectedChildrenChangedNotification]{
            _=AXObserverAddNotification(observer,root,event as CFString,nil)
        }
        CFRunLoopAddSource(CFRunLoopGetMain(),AXObserverGetRunLoopSource(observer),.defaultMode)
        observers[pid]=observer
    }
    func focused(_ t:RefRecord<AXUIElement?>)throws->Bool{
        try requireAX()

        guard let window=t.value,launch(t.pid)==t.launch else{return false}
        let system=AXUIElementCreateSystemWide()
        guard let current=try? axElement(axRaw(system,kAXFocusedApplicationAttribute)) else{return false}
        var livePid:pid_t=0
        guard AXUIElementGetPid(current,&livePid) == .success,livePid==t.pid else{return false}

        let root=AXUIElementCreateApplication(t.pid)
        guard let raw=try? axRaw(root,kAXFocusedWindowAttribute),let actual=try? axElement(raw) else{return false}
        return CFEqual(actual,window)
    }
    func windows(_ r:SWRequest)throws->[String:Any]{
        try requireAX();var rows:[[String:Any]]=[];var partial=false
        for app in apps(){
            try r.checkpoint();observeApp(app)
            guard let (list,total)=try? axElements(AXUIElementCreateApplication(app.processIdentifier),kAXWindowsAttribute,limit:64) else{partial=true;continue}
            if list.count<total{partial=true}
            for w in list {
                try r.checkpoint();if rows.count>=512{partial=true;break}
                let s=try stamp("win",app,w)
                var row:[String:Any]=["ref":["$ref":s.json],"app":appID(app),"pid":app.processIdentifier,"title":(try? axString(w,kAXTitleAttribute,limit:512)) ?? ""]
                if let b=try? axBounds(w){row["bounds"]=b.json}
                rows.append(row)
            }
        }
        return["windows":rows,"partial":partial]
    }
    func hitTest(_ r:SWRequest)throws->[String:Any]{
        try requireAX()
        let x=try number(r.args["x"],default:0,range:-1_000_000...1_000_000)
        let y=try number(r.args["y"],default:0,range:-1_000_000...1_000_000)
        var hit:AXUIElement?
        try axCheck(AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(),Float(x),Float(y),&hit))
        guard let e=hit else{throw SWFailure(code:"NotFound",message:"No accessibility object at screen point")}
        var pid:pid_t=0;try axCheck(AXUIElementGetPid(e,&pid))
        guard let app=NSRunningApplication(processIdentifier:pid),app.launchDate != nil else{
            throw SWFailure(code:"StaleReference",message:"Hit-tested application disappeared")
        }
        let raw=try axString(e,kAXRoleAttribute,limit:80)
        let secure=axSecure(e)
        let stamp=try self.stamp(raw=="AXWindow" ? "win":"ui",app,e)
        var actions:[String]=[]
        if !secure {
            var names:CFArray?
            if AXUIElementCopyActionNames(e,&names) == .success,let values=names as? [String]{
                if values.contains(kAXPressAction){actions.append("click")}
                if values.contains(kAXIncrementAction){actions.append("increment")}
                if values.contains(kAXDecrementAction){actions.append("decrement")}
                if values.contains(kAXShowMenuAction){actions.append("show_menu")}
            }
        }
        let name=secure ? "" : ((try? axString(e,kAXTitleAttribute,limit:1024)) ?? "")
        let description=secure ? "" : ((try? axString(e,kAXDescriptionAttribute,limit:1024)) ?? "")
        let help=secure ? "" : ((try? axString(e,kAXHelpAttribute,limit:1024)) ?? "")
        let accessibilityID=secure ? "" : ((try? axString(e,kAXIdentifierAttribute,limit:512)) ?? "")
        let subrole=(try? axString(e,kAXSubroleAttribute,limit:128)) ?? ""
        var attributes:[String:String]=["ax_role":raw]
        if !subrole.isEmpty{attributes["ax_subrole"]=subrole}
        let childCount=axArrayCount(e,kAXChildrenAttribute) ?? 0
        var facets:[String:Any]=[:]
        if let count=axOptionalNumber(e,kAXNumberOfCharactersAttribute){
            facets["text"]=[
                "character_count":max(0,Int(count)),
                "selection_count":axArrayCount(e,kAXSelectedTextRangesAttribute) as Any,
                "editable":axBool(e,kAXIsEditableAttribute),
                "password":secure,
            ]
        }
        if let current=axOptionalNumber(e,kAXValueAttribute){
            var value:[String:Any]=["current":current]
            if let minimum=axOptionalNumber(e,kAXMinValueAttribute){value["minimum"]=minimum}
            if let maximum=axOptionalNumber(e,kAXMaxValueAttribute){value["maximum"]=maximum}
            if let increment=axOptionalNumber(e,kAXValueIncrementAttribute){value["increment"]=increment}
            facets["value"]=value
        }
        let selectedChildren=axArrayCount(e,kAXSelectedChildrenAttribute)
        if selectedChildren != nil || axBool(e,kAXSelectedAttribute){
            var selection:[String:Any]=["selected":axBool(e,kAXSelectedAttribute),"child_count":min(childCount,2000)]
            if let selectedChildren=selectedChildren{selection["selected_count"]=selectedChildren}
            facets["selection"]=selection
        }
        if raw=="AXWindow"{
            facets["window"]=[
                "modal":axBool(e,kAXModalAttribute),
                "minimized":axBool(e,kAXMinimizedAttribute),
            ]
            facets["transform"]=[
                "can_move":axSettable(e,kAXPositionAttribute) as Any,
                "can_resize":axSettable(e,kAXSizeAttribute) as Any,
            ]
        }
        var relations:[[String:Any]]=[]
        if let labelRaw=try? axRaw(e,kAXTitleUIElementAttribute),
           let label=try? axElement(labelRaw),
           let labelStamp=try? self.stamp("ui",app,label){
            relations.append(["kind":"labelled_by","targets":[["$ref":labelStamp.json]]])
        }
        var node:[String:Any]=[
            "ref":["$ref":stamp.json],
            "role":Semantic.role(raw),
            "name":name,
            "description":description,
            "help":help,
            "accessibility_id":accessibilityID,
            "framework":"appkit-ax",
            "attributes":attributes,
            "relations":relations,
            "facets":facets,
            "states":Semantic.states(enabled:axBool(e,kAXEnabledAttribute),focused:axBool(e,kAXFocusedAttribute),selected:axBool(e,kAXSelectedAttribute),expanded:axBool(e,kAXExpandedAttribute)),
            "actions":actions,
            "app":appID(app),
            "parent_ref":NSNull(),
            "bounds":NSNull(),
            "children_count":min(childCount,2000),
        ]
        if let b=try? axBounds(e){node["bounds"]=b.json}
        return[
            "node":node,
            "point":["x":x,"y":y,"coordinate_space":"screen"],
            "semantic_coverage":"native_hit_test",
        ]
    }

    func snapshot(_ r:SWRequest)throws->[String:Any]{
        try requireAX()
        let maxNodes=Int(try number(r.args["max_nodes"],default:500,range:1...2000))
        let maxDepth=Int(try number(r.args["max_depth"],default:8,range:0...32))
        let appFilter=r.args["app"] as? String
        var stack:[(AXUIElement,NSRunningApplication,Int,RefStamp?)]=[]
        for app in apps() where appFilter==nil || appID(app)==appFilter {
            observeApp(app);stack.append((AXUIElementCreateApplication(app.processIdentifier),app,0,nil))
        }
        var rows:[[String:Any]]=[];var seen:[AXUIElement]=[];var partial=false;var used=128
        while let (e,app,depth,parent)=stack.popLast(){
            try r.checkpoint()
            if seen.count>=maxNodes{partial=true;break}
            if seen.contains(where:{CFEqual($0,e)}){continue};seen.append(e)
            guard let raw=try? axString(e,kAXRoleAttribute,limit:80) else{partial=true;continue}
            let secure=axSecure(e)
            let s=try stamp(raw=="AXWindow" ? "win":"ui",app,e)
            var actions:[String]=[]
            if !secure {
                var names:CFArray?
                if AXUIElementCopyActionNames(e,&names) == .success,let values=names as? [String]{
                    // Action names are an allowlist; application strings are never executed as code.
                    if values.contains(kAXPressAction){actions.append("click")}
                    if values.contains(kAXIncrementAction){actions.append("increment")}
                    if values.contains(kAXDecrementAction){actions.append("decrement")}
                    if values.contains(kAXShowMenuAction){actions.append("show_menu")}
                }
            }
            var children:[AXUIElement]=[];var total=0
            if !secure,let tuple=try? axElements(e,kAXChildrenAttribute,limit:128){children=tuple.0;total=tuple.1}
            if total>children.count{partial=true}
            if depth>=maxDepth && total>0{partial=true;children=[]}
            let name=secure ? "":((try? axString(e,kAXTitleAttribute,limit:1024)) ?? "")
            let description=secure ? "":((try? axString(e,kAXDescriptionAttribute,limit:512)) ?? "")
            let help=secure ? "":((try? axString(e,kAXHelpAttribute,limit:1024)) ?? "")
            let accessibilityID=secure ? "":((try? axString(e,kAXIdentifierAttribute,limit:512)) ?? "")
            let subrole=(try? axString(e,kAXSubroleAttribute,limit:128)) ?? ""
            var attributes:[String:String]=["ax_role":raw]
            if !subrole.isEmpty{attributes["ax_subrole"]=subrole}
            var facets:[String:Any]=[:]
            if let count=axOptionalNumber(e,kAXNumberOfCharactersAttribute){
                let selections=axArrayCount(e,kAXSelectedTextRangesAttribute)
                    ?? ((try? axRaw(e,kAXSelectedTextRangeAttribute)) == nil ? nil : 1)
                facets["text"]=[
                    "character_count":max(0,Int(count)),
                    "selection_count":selections as Any,
                    "editable":axBool(e,kAXIsEditableAttribute),
                    "password":secure,
                ]
            }
            if let current=axOptionalNumber(e,kAXValueAttribute){
                var value:[String:Any]=["current":current]
                if let minimum=axOptionalNumber(e,kAXMinValueAttribute){value["minimum"]=minimum}
                if let maximum=axOptionalNumber(e,kAXMaxValueAttribute){value["maximum"]=maximum}
                if let increment=axOptionalNumber(e,kAXValueIncrementAttribute){value["increment"]=increment}
                facets["value"]=value
            }
            let selectedChildren=axArrayCount(e,kAXSelectedChildrenAttribute)
            if selectedChildren != nil || axBool(e,kAXSelectedAttribute){
                var selection:[String:Any]=["selected":axBool(e,kAXSelectedAttribute)]
                if let selectedChildren=selectedChildren{selection["selected_count"]=selectedChildren}
                selection["child_count"]=min(total,2000)
                facets["selection"]=selection
            }
            if raw=="AXWindow"{
                facets["window"]=[
                    "modal":axBool(e,kAXModalAttribute),
                    "minimized":axBool(e,kAXMinimizedAttribute),
                ]
                facets["transform"]=[
                    "can_move":axSettable(e,kAXPositionAttribute) as Any,
                    "can_resize":axSettable(e,kAXSizeAttribute) as Any,
                ]
            }
            var node:[String:Any]=[
                "ref":["$ref":s.json],
                "role":Semantic.role(raw),
                "name":name,
                "description":description,
                "help":help,
                "accessibility_id":accessibilityID,
                "framework":"appkit-ax",
                "attributes":attributes,
                "facets":facets,
                "states":Semantic.states(enabled:axBool(e,kAXEnabledAttribute),focused:axBool(e,kAXFocusedAttribute),selected:axBool(e,kAXSelectedAttribute),expanded:axBool(e,kAXExpandedAttribute)),
                "actions":actions,
                "app":appID(app),
                "children_count":min(total,2000),
                "parent_ref":parent.map{["$ref":$0.json]} as Any? ?? NSNull(),
                "bounds":NSNull()
            ]
            if let b=try? axBounds(e){node["bounds"]=b.json}
            let size=(try? JSONSerialization.data(withJSONObject:node).count) ?? 1_048_576
            if used+size>900_000{partial=true;break};used+=size
            if r.args["actionable"] as? Bool != true || !actions.isEmpty{rows.append(node)}
            // Parent refs always resolve even when actionable filtering hides the parent row.
            for child in children.reversed(){stack.append((child,app,depth+1,s))}
        }
        revision &+= 1
        return["nodes":rows,"revision":revision,"partial":partial]
    }
    func windowAction(_ r:SWRequest)throws->[String:Any]{
        try requireAX();let t=try resolve(r.args,kind:"win")
        guard let w=t.value,let app=NSRunningApplication(processIdentifier:t.pid) else{throw SWFailure(code:"StaleReference",message:"Window disappeared")}
        switch r.command {
        case "window.focus":
            try r.mutation();try axCheck(AXUIElementPerformAction(w,kAXRaiseAction as CFString))
            try require(app.activate(options:[]),"BackendFailed","Application activation failed")
            // Do not claim focus merely because activation was requested.
            let verified=try focused(t);return["requested":true,"focused":verified]
        case "window.move":
            var p=CGPoint(x:try number(r.args["x"],default:0,range:-32768...32767),y:try number(r.args["y"],default:0,range:-32768...32767))
            guard let value=AXValueCreate(.cgPoint,&p) else{throw SWFailure(code:"Internal",message:"Geometry allocation failed")}
            try r.mutation();try axSet(w,kAXPositionAttribute,value)
        case "window.resize":
            var s=CGSize(width:try number(r.args["width"],default:1,range:1...16384),height:try number(r.args["height"],default:1,range:1...16384))
            guard let value=AXValueCreate(.cgSize,&s) else{throw SWFailure(code:"Internal",message:"Geometry allocation failed")}
            try r.mutation();try axSet(w,kAXSizeAttribute,value)
        case "window.close":
            let button=try axElement(axRaw(w,kAXCloseButtonAttribute))
            try r.mutation();try axCheck(AXUIElementPerformAction(button,kAXPressAction as CFString))
        default:throw SWFailure(code:"Unsupported",message:"Unknown window action")
        }
        ledger.invalidate(t.pid);return["applied":true]
    }
    func uiAction(_ r:SWRequest)throws->[String:Any]{
        try requireAX();let t=try resolve(r.args)
        try require(t.stamp.kind=="ui" || t.stamp.kind=="win","StaleReference","Semantic UI ref required")
        guard let e=t.value else{throw SWFailure(code:"StaleReference",message:"UI object missing")}
        try require(!axSecure(e),"PermissionDenied","Secure Accessibility fields are not read or changed")
        switch r.command {
        case "ui.read_text":
            let count=Int(try number(r.args["max_chars"],default:4096,range:1...65536))
            // AX ranges use UTF-16 units. Query the count before requesting a range;
            // requesting max_chars past the end is rejected by many normal controls.
            let availableRaw=try axRaw(e,kAXNumberOfCharactersAttribute)
            guard let available=availableRaw as? NSNumber,
                  CFGetTypeID(available) != CFBooleanGetTypeID(),
                  available.doubleValue.isFinite,available.doubleValue>=0,
                  available.doubleValue.rounded(.towardZero)==available.doubleValue,
                  available.doubleValue<Double(Int.max) else{
                throw SWFailure(code:"Unsupported",message:"Bounded text length unavailable")
            }
            let length=min(count,available.intValue)
            if length==0{return["text":"","max_utf16_units":count,"truncated":false]}
            var range=CFRange(location:0,length:length)
            guard let arg=AXValueCreate(.cfRange,&range) else{throw SWFailure(code:"Internal",message:"Text range allocation failed")}
            var raw:CFTypeRef?
            try axCheck(AXUIElementCopyParameterizedAttributeValue(e,kAXStringForRangeParameterizedAttribute as CFString,arg,&raw))
            guard let raw=raw,CFGetTypeID(raw)==CFStringGetTypeID() else{throw SWFailure(code:"Unsupported",message:"Bounded text retrieval unsupported")}
            return["text":bounded(raw as! String,count*4),"max_utf16_units":count,"truncated":available.intValue>length]
        case "ui.get_value":
            let value=try axNumber(e,kAXValueAttribute)
            let limits=try axNumericRange(e)
            try require(limits.contains(value),"Conflict","Accessible value is outside its reported range")
            return["value":value,"minimum":limits.lowerBound,"maximum":limits.upperBound]
        case "ui.set_value":
            let value=try number(r.args["value"],default:0,range:-1e9...1e9)
            let limits=try axNumericRange(e)
            try require(limits.contains(value),"InvalidArgument","Value exceeds the control's reported range")
            try r.mutation();try axSet(e,kAXValueAttribute,NSNumber(value:value))
        case "ui.set_text":
            guard let text=r.args["text"] as? String else{throw SWFailure(code:"InvalidArgument",message:"Text required")}
            try require(text.utf8.count<=65536);try r.mutation();try axSet(e,kAXValueAttribute,text as CFString)
        case "ui.expand":try r.mutation();try axSet(e,kAXExpandedAttribute,kCFBooleanTrue)
        case "ui.toggle":
            let role=try axString(e,kAXRoleAttribute,limit:80)
            try require(role=="AXCheckBox" || role=="AXRadioButton","Unsupported","Toggle requires a semantic selection control")
            try axAdvertises(e,kAXPressAction);try r.mutation();try axCheck(AXUIElementPerformAction(e,kAXPressAction as CFString))
        case "ui.invoke":
            let name=(r.args["action"] as? String) ?? "click"
            let allowed=["click":kAXPressAction,"increment":kAXIncrementAction,"decrement":kAXDecrementAction,"show_menu":kAXShowMenuAction]
            guard let action=allowed[name] else{throw SWFailure(code:"Unsupported",message:"Semantic action is not allowlisted")}
            try axAdvertises(e,action);try r.mutation();try axCheck(AXUIElementPerformAction(e,action as CFString))
        default:throw SWFailure(code:"Unsupported",message:"Unknown semantic UI operation")
        }
        ledger.invalidate(t.pid);return["applied":true]
    }
}

// These helpers accept only a numeric AX value; CFBoolean must never masquerade as 0/1.
private func axNumber(_ e:AXUIElement,_ key:String)throws->Double{
    let raw=try axRaw(e,key)
    guard let n=raw as? NSNumber,CFGetTypeID(n) != CFBooleanGetTypeID(),n.doubleValue.isFinite else{
        throw SWFailure(code:"Unsupported",message:"Numeric Accessibility attribute unavailable")
    }
    return n.doubleValue
}
private func axNumericRange(_ e:AXUIElement)throws->ClosedRange<Double>{
    let lower=try axNumber(e,kAXMinValueAttribute),upper=try axNumber(e,kAXMaxValueAttribute)
    try require(lower<=upper,"Unsupported","Control reported an invalid numeric range")
    return lower...upper
}
private func axAdvertises(_ e:AXUIElement,_ action:String)throws{
    var names:CFArray?
    try axCheck(AXUIElementCopyActionNames(e,&names))
    guard let names=names as? [String],names.contains(action) else{
        throw SWFailure(code:"Unsupported",message:"Control does not advertise the requested action")
    }
}
