import Foundation
import AppKit
import ApplicationServices
import CoreGraphics
import CSemwrightNative
private func secureInputActive()->Bool{sw_secure_input_active()}

@MainActor extension NativeHost {
    func inputGuard(_ request:SWRequest,_ target:RefRecord<AXUIElement?>)throws{
        try request.checkpoint();try requireAX()
        try require(CGPreflightPostEventAccess(),"PermissionDenied","Event posting is not authorized")
        try require(!secureInputActive(),"PermissionDenied","Secure Event Input is active")
        try require(try focused(target),"Conflict","Target window is no longer focused; no input was posted")
        // Live AX state, not cached NSRunningApplication activation properties.
        let app=AXUIElementCreateApplication(target.pid)
        let focus=try axElement(axRaw(app,kAXFocusedUIElementAttribute))
        try require(!axSecure(focus),"PermissionDenied","Focused control is secure or cannot be validated")
    }
    func pointerGuard(_ point:CGPoint,_ request:SWRequest,_ target:RefRecord<AXUIElement?>)throws{
        try inputGuard(request,target)
        guard let expected=target.value else{throw SWFailure(code:"StaleReference",message:"Window disappeared")}
        var hit:AXUIElement?
        try axCheck(AXUIElementCopyElementAtPosition(AXUIElementCreateSystemWide(),Float(point.x),Float(point.y),&hit))
        guard let hit=hit else{throw SWFailure(code:"Unavailable",message:"Pointer destination cannot be validated")}
        var pid:pid_t=0
        try axCheck(AXUIElementGetPid(hit,&pid))
        try require(pid==target.pid && !axSecure(hit),"Conflict","Pointer target is not the expected nonsecure application")
        if !CFEqual(hit,expected){
            let window=try axElement(axRaw(hit,kAXWindowAttribute))
            try require(CFEqual(window,expected),"Conflict","Another window covers the pointer destination")
        }
        // Validation and CGEvent submission are separate OS calls. The result still
        // reports delivery_verified=false; this is not an atomic delivery guarantee.
    }
    func postKey(_ key:CGKeyCode,_ units:[UInt16]?,_ request:SWRequest,_ target:RefRecord<AXUIElement?>)throws{
        try inputGuard(request,target)
        guard let down=CGEvent(keyboardEventSource:nil,virtualKey:key,keyDown:true),let up=CGEvent(keyboardEventSource:nil,virtualKey:key,keyDown:false) else{throw SWFailure(code:"BackendFailed",message:"Keyboard event allocation failed")}
        if let units=units{
            units.withUnsafeBufferPointer{p in down.keyboardSetUnicodeString(stringLength:p.count,unicodeString:p.baseAddress);up.keyboardSetUnicodeString(stringLength:p.count,unicodeString:p.baseAddress)}
        }
        try request.mutation()
        // A release is paired even on cancellation. No held input state crosses requests.
        defer{up.post(tap:.cghidEventTap)}
        down.post(tap:.cghidEventTap)
    }
    func input(_ r:SWRequest)throws->[String:Any]{
        let t=try resolve(r.args,kind:"win")
        try inputGuard(r,t)
        switch r.command{
        case "input.type":
            guard let text=r.args["text"] as? String else{throw SWFailure(code:"InvalidArgument",message:"Text required")}
            let chunks=try Semantic.unicodeChunks(text)
            for chunk in chunks{try postKey(0,chunk,r,t)}
            return["posted":true,"delivery_verified":false,"utf16_units":text.utf16.count]
        case "input.key":
            let value=try number(r.args["keysym"],default:0,range:0...4294967295)
            try require(value.rounded(.towardZero)==value)
            let key=UInt32(value)
            // The existing public v1 schema uses keysym. Preserve its interpretation;
            // never reinterpret those integers as macOS hardware key codes.
            let special:[UInt32:CGKeyCode]=[0xff0d:36,0xff09:48,0xff1b:53,0xff08:51,0xffff:117,0xff51:123,0xff53:124,0xff52:126,0xff54:125,0xff50:115,0xff57:119,0xff55:116,0xff56:121]
            if let code=special[key]{try postKey(code,nil,r,t)}
            else {
                let scalar=key&0xff000000 == 0x01000000 ? key&0x00ffffff : key
                guard let u=UnicodeScalar(scalar),scalar>=32,scalar<0xff00 || key&0xff000000==0x01000000 else{throw SWFailure(code:"Unsupported",message:"Keysym mapping unavailable; use Unicode input.type")}
                try postKey(0,Array(String(u).utf16),r,t)
            }
        case "pointer.move","pointer.click","pointer.scroll":
            guard let window=t.value,let current=CGEvent(source:nil)?.location else{throw SWFailure(code:"Unavailable",message:"Pointer/window geometry unavailable")}
            let bounds=try axBounds(window)
            var point=current
            if r.command=="pointer.move"{
                point.x += try number(r.args["dx"],default:0,range:-10000...10000)
                point.y += try number(r.args["dy"],default:0,range:-10000...10000)
            }
            try require(bounds.contains(point.x,point.y),"PolicyDenied","Pointer destination is outside the explicit focused window")
            try inputGuard(r,t)
            if r.command=="pointer.move"{
                guard let event=CGEvent(mouseEventSource:nil,mouseType:.mouseMoved,mouseCursorPosition:point,mouseButton:.left) else{throw SWFailure(code:"BackendFailed",message:"Pointer event allocation failed")}
                try pointerGuard(point,r,t);try r.mutation();event.post(tap:.cghidEventTap)
            }else if r.command=="pointer.click"{
                let name=r.args["button"] as? String
                let button:CGMouseButton,downType:CGEventType,upType:CGEventType
                switch name{case "left":button = .left;downType = .leftMouseDown;upType = .leftMouseUp
                    case "right":button = .right;downType = .rightMouseDown;upType = .rightMouseUp
                    case "middle":button = .center;downType = .otherMouseDown;upType = .otherMouseUp
                    default:throw SWFailure(code:"InvalidArgument",message:"Unknown mouse button")}
                guard let down=CGEvent(mouseEventSource:nil,mouseType:downType,mouseCursorPosition:point,mouseButton:button),let up=CGEvent(mouseEventSource:nil,mouseType:upType,mouseCursorPosition:point,mouseButton:button) else{throw SWFailure(code:"BackendFailed",message:"Pointer event allocation failed")}
                try pointerGuard(point,r,t);try r.mutation();defer{up.post(tap:.cghidEventTap)};down.post(tap:.cghidEventTap)
            }else{
                let dx=try number(r.args["dx"],default:0,range:-1000...1000),dy=try number(r.args["dy"],default:0,range:-1000...1000)
                // v1 deltas are not display pixels: explicitly use line units and reject
                // fractional line requests rather than silently truncate.
                try require(dx.rounded(.towardZero)==dx && dy.rounded(.towardZero)==dy,"Unsupported","Fractional scroll lines are not exposed")
                guard let event=CGEvent(scrollWheelEvent2Source:nil,units:.line,wheelCount:2,wheel1:Int32(dy),wheel2:Int32(dx),wheel3:0) else{throw SWFailure(code:"BackendFailed",message:"Scroll event allocation failed")}
                try pointerGuard(point,r,t);try r.mutation();event.post(tap:.cghidEventTap)
            }
        default:throw SWFailure(code:"Unsupported",message:"Unknown input operation")
        }
        return["posted":true,"delivery_verified":false]
    }
}
