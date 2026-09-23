import Foundation
import AppKit
import ScreenCaptureKit
import CoreGraphics
import ImageIO
import UniformTypeIdentifiers
import Darwin

@MainActor final class ArtifactStore {
    let root:Int32;let path:String
    private var entries:[String:Double]=[:]
    private var timer:Timer?
    init(path:String)throws{
        try require(path.hasPrefix("/") && path.utf8.count<4096)
        root=open(path,O_RDONLY|O_DIRECTORY|O_NOFOLLOW|O_CLOEXEC)
        try require(root>=0,"PermissionDenied","Private artifact directory unavailable")
        var st=stat()
        if fstat(root,&st) != 0 || st.st_uid != getuid() || st.st_mode&0o777 != 0o700{
            close(root);throw SWFailure(code:"PermissionDenied",message:"Artifact directory must be private mode 0700")
        }
        self.path=path
        timer=Timer.scheduledTimer(withTimeInterval:5,repeats:true){[weak self]_ in Task{@MainActor in self?.prune()}}
    }
    func prune(){let now=ProcessInfo.processInfo.systemUptime;for(k,v) in entries where v<=now{_ = unlinkat(root,k,0);entries.removeValue(forKey:k)}}
    func clear(){for k in entries.keys{_ = unlinkat(root,k,0)};entries.removeAll();timer?.invalidate()}
    deinit{close(root)}
    func store(_ image:CGImage,_ r:SWRequest)throws->[String:Any]{
        prune();try require(entries.count<16,"ResourceExhausted","Capture artifact capacity exhausted")
        try require(image.width>0 && image.height>0 && image.width<=4096 && image.height<=4096 && image.width*image.height<=4_194_304,"ResourceExhausted","Capture pixel budget exceeded")
        let data=NSMutableData()
        guard let writer=CGImageDestinationCreateWithData(data,UTType.png.identifier as CFString,1,nil) else{throw SWFailure(code:"BackendFailed",message:"PNG encoder unavailable")}
        CGImageDestinationAddImage(writer,image,nil)
        try require(CGImageDestinationFinalize(writer),"BackendFailed","PNG encoding failed")
        try require(data.length<=8_388_608,"ResourceExhausted","Capture byte budget exceeded")
        try r.checkpoint()
        let name="capture-\(UUID().uuidString.lowercased()).png"
        let fd=openat(root,name,O_WRONLY|O_CREAT|O_EXCL|O_NOFOLLOW|O_CLOEXEC,0o600)
        try require(fd>=0,"BackendFailed","Cannot create private capture artifact")
        var ok=false;defer{close(fd);if !ok{_ = unlinkat(root,name,0)}}
        var offset=0
        while offset<data.length{
            try r.checkpoint()
            let n=write(fd,data.bytes.advanced(by:offset),data.length-offset)
            if n<0 && errno==EINTR{continue}
            try require(n>0,"BackendFailed","Capture artifact write failed");offset+=n
        }
        try require(fsync(fd)==0,"BackendFailed","Capture artifact sync failed")
        entries[name]=ProcessInfo.processInfo.systemUptime+60;ok=true
        return["path":path+"/"+name,"mime_type":"image/png","bytes":data.length,"width":image.width,"height":image.height,"expires_in_seconds":60]
    }
}
@MainActor private final class PickerLease:NSObject,SCContentSharingPickerObserver {
    private var continuation:CheckedContinuation<SCContentFilter,Error>?
    private var timer:Timer?
    private var finished=false
    func pick(_ r:SWRequest)async throws->SCContentFilter{
        let picker=SCContentSharingPicker.shared
        let requestID=r.id
        picker.add(self);picker.isActive=true
        return try await withCheckedThrowingContinuation{continuation in
            self.continuation=continuation
            self.timer=Timer.scheduledTimer(withTimeInterval:0.1,repeats:true){[weak self]_ in
                Task{@MainActor in do{try CancellationRegistry.shared.check(requestID)}catch{self?.finish(.failure(error))}}
            }
            picker.present()
        }
    }
    func finish(_ value:Result<SCContentFilter,Error>){
        guard !finished else{return};finished=true
        timer?.invalidate();timer=nil
        let picker=SCContentSharingPicker.shared;picker.remove(self);picker.isActive=false
        let c=continuation;continuation=nil;c?.resume(with:value)
    }
    nonisolated func contentSharingPicker(_ picker:SCContentSharingPicker,didCancelFor stream:SCStream?){
        Task{@MainActor in self.finish(.failure(SWFailure(code:"Cancelled",message:"User cancelled screen selection")))}
    }
    nonisolated func contentSharingPicker(_ picker:SCContentSharingPicker,didUpdateWith filter:SCContentFilter,for stream:SCStream?){
        guard stream==nil else{return}
        Task{@MainActor in self.finish(.success(filter))}
    }
    nonisolated func contentSharingPickerStartDidFailWithError(_ error:Error){
        Task{@MainActor in self.finish(.failure(SWFailure(code:"PermissionDenied",message:"System screen picker could not start")))}
    }
}
@MainActor extension NativeHost {
    func capture(_ r:SWRequest)async throws->[String:Any]{
        try require(!captureBusy,"Conflict","A screen selection is already in progress")
        guard let artifacts=artifacts else{throw SWFailure(code:"Unavailable",message:"Capture storage is not configured")}
        captureBusy=true;defer{captureBusy=false}
        // Existing v1 screen.capture has no target argument. Do not silently capture
        // the entire desktop: the system picker supplies an explicit owner selection.
        let lease=PickerLease();let filter=try await lease.pick(r)
        try r.checkpoint()
        let rect=filter.contentRect;let scale=Double(filter.pointPixelScale)
        try require(rect.width.isFinite && rect.height.isFinite && rect.width>0 && rect.height>0 && scale.isFinite && scale>0,"Unavailable","Selected capture geometry is invalid")
        let factor=min(1.0,sqrt(4_194_304/(Double(rect.width*rect.height)*scale*scale)),4096/(Double(rect.width)*scale),4096/(Double(rect.height)*scale))
        let config=SCStreamConfiguration()
        config.width=max(1,Int(floor(Double(rect.width)*scale*factor)))
        config.height=max(1,Int(floor(Double(rect.height)*scale*factor)))
        config.showsCursor=false;config.capturesAudio=false
        let image=try await SCScreenshotManager.captureImage(contentFilter:filter,configuration:config)
        try r.checkpoint()
        var result=try artifacts.store(image,r)
        result["selection"]="system_content_picker"
        result["coordinate_space"]="screencapturekit_selected_content_points"
        result["logical_bounds"]=["x":rect.minX,"y":rect.minY,"width":rect.width,"height":rect.height]
        result["point_pixel_scale"]=scale;result["output_scale"]=scale*factor
        result["display_identity"]="not_exposed_by_v1_picker_contract"
        return result
    }
}
