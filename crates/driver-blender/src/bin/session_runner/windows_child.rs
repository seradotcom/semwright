//! Closed native child launch: synchronous anonymous pipes and exactly three
//! inherited handles. Child stays suspended until the runner attests its token/Job.
use std::{
    ffi::OsStr, fs::File, io, mem::size_of,
    os::windows::{ffi::OsStrExt, io::{AsRawHandle, FromRawHandle, OwnedHandle, RawHandle},
        process::ExitStatusExt},
    process::{Command, ExitStatus},
};
#[path = "windows_launch_diagnostics.rs"]
mod diagnostics;
use windows_sys::Win32::{
    Foundation::{DuplicateHandle, SetHandleInformation, DUPLICATE_SAME_ACCESS,
        HANDLE, HANDLE_FLAG_INHERIT, WAIT_OBJECT_0},
    Security::{SECURITY_ATTRIBUTES, SECURITY_CAPABILITIES},
    System::{
        Pipes::CreatePipe,
        WindowsProgramming::PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT,
        Threading::{CreateProcessW, DeleteProcThreadAttributeList, GetCurrentProcess,
            GetExitCodeProcess, InitializeProcThreadAttributeList, ResumeThread,
            TerminateProcess, UpdateProcThreadAttribute, WaitForSingleObject,
            CREATE_NO_WINDOW, CREATE_SUSPENDED, CREATE_UNICODE_ENVIRONMENT,
            EXTENDED_STARTUPINFO_PRESENT, PROCESS_INFORMATION,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST, PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES,
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY,
            STARTF_USESTDHANDLES, STARTUPINFOEXW},
    },
};
fn invalid() -> io::Error { io::Error::new(io::ErrorKind::InvalidInput, "Invalid fixed native launch data") }
fn wide(value: &OsStr) -> io::Result<Vec<u16>> {
    let mut result: Vec<u16> = value.encode_wide().collect();
    if result.contains(&0) || result.len() >= 32_766 { return Err(invalid()); }
    result.push(0);Ok(result)
}
fn quote(value: &OsStr, out: &mut Vec<u16>) -> io::Result<()> {
    let units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) { return Err(invalid()); }
    out.push(34);
    let mut slashes = 0;
    for unit in units {
        if unit == 92 { slashes += 1;continue; }
        if unit == 34 {
            out.extend(std::iter::repeat_n(92, slashes*2+1));
        } else {
            out.extend(std::iter::repeat_n(92, slashes));
        }
        slashes = 0;out.push(unit);
    }
    out.extend(std::iter::repeat_n(92, slashes*2));
    out.push(34);Ok(())
}
fn command_line(command: &Command) -> io::Result<Vec<u16>> {
    let mut out=Vec::new();quote(command.get_program(), &mut out)?;
    for arg in command.get_args() { out.push(32);quote(arg,&mut out)?; }
    if out.len() >= 32_766 { return Err(invalid()); }
    out.push(0);Ok(out)
}
fn environment(command: &Command) -> io::Result<Vec<u16>> {
    let mut entries=Vec::new();
    for (name,value) in command.get_envs() {
        let value=value.ok_or_else(invalid)?;
        let key=name.to_str().ok_or_else(invalid)?;
        if key.is_empty() || key.contains('=') || !key.is_ascii() {return Err(invalid());}
        let mut row=wide(name)?;row.pop();row.push(61);
        let mut data=wide(value)?;data.pop();row.extend(data);row.push(0);
        entries.push((key.to_ascii_uppercase(),row));
    }
    if entries.is_empty() {return Err(invalid());}
    entries.sort_by(|a,b|a.0.cmp(&b.0));
    let mut out=Vec::new();
    for (_,entry) in entries {out.extend(entry);}
    out.push(0);
    if out.len()>=32_767 {return Err(invalid());}Ok(out)
}
fn pipe() -> io::Result<(File,File)> {
    let mut read=std::ptr::null_mut();
    let mut write=std::ptr::null_mut();
    let attrs=SECURITY_ATTRIBUTES {nLength:size_of::<SECURITY_ATTRIBUTES>() as u32,
        lpSecurityDescriptor:std::ptr::null_mut(),bInheritHandle:1};
    // SAFETY: writable handle outputs and fixed lifetime security attributes.
    if unsafe {CreatePipe(&mut read,&mut write,&attrs,65_536)}==0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: successful CreatePipe transfers ownership of two new handles.
    Ok(unsafe {(File::from_raw_handle(read),File::from_raw_handle(write))})
}
fn noninheritable(file:&File)->io::Result<()> {
    // SAFETY: owned live handle; clear only its inheritance flag.
    if unsafe {SetHandleInformation(file.as_raw_handle(),HANDLE_FLAG_INHERIT,0)}==0 {
        return Err(io::Error::last_os_error());
    }Ok(())
}
fn inherited_log(file:&File)->io::Result<OwnedHandle>{
    let mut duplicate=std::ptr::null_mut();
    // SAFETY: duplicate this owned file only, preserving access and setting inheritance.
    if unsafe {DuplicateHandle(GetCurrentProcess(),file.as_raw_handle(),GetCurrentProcess(),
        &mut duplicate,0,1,DUPLICATE_SAME_ACCESS)}==0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: DuplicateHandle returned a newly owned handle.
    Ok(unsafe {OwnedHandle::from_raw_handle(duplicate)})
}
struct Attributes {storage:Vec<usize>}
impl Attributes {
    fn new(handles:&[HANDLE;3],capabilities:&SECURITY_CAPABILITIES,
        opt_out:&u32)->io::Result<Self>{
        let mut size=0usize;
        // SAFETY: documented size-only query with null attribute list.
        unsafe {InitializeProcThreadAttributeList(std::ptr::null_mut(),3,0,&mut size);}
        if size==0 || size>65_536 {return Err(invalid());}
        let mut storage=vec![0usize;size.div_ceil(size_of::<usize>())];
        // SAFETY: aligned allocation has the requested capacity and remains live.
        if unsafe {InitializeProcThreadAttributeList(storage.as_mut_ptr().cast(),3,0,&mut size)}==0 {
            return Err(io::Error::last_os_error());
        }
        let result=Self{storage};
        // SAFETY: list initialized above; exact owned handles stay alive through creation.
        if unsafe {UpdateProcThreadAttribute(result.storage.as_ptr().cast_mut().cast(),0,
            PROC_THREAD_ATTRIBUTE_HANDLE_LIST as usize,handles.as_ptr().cast_mut().cast(),
            std::mem::size_of_val(handles),std::ptr::null_mut(),std::ptr::null_mut())}==0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: both fixed attribute values live in spawn until creation completes.
        if unsafe{UpdateProcThreadAttribute(result.storage.as_ptr().cast_mut().cast(),0,
            PROC_THREAD_ATTRIBUTE_SECURITY_CAPABILITIES as usize,
            (capabilities as *const SECURITY_CAPABILITIES).cast_mut().cast(),
            size_of::<SECURITY_CAPABILITIES>(),std::ptr::null_mut(),std::ptr::null_mut())}==0 {
            return Err(io::Error::last_os_error());
        }
        if unsafe{UpdateProcThreadAttribute(result.storage.as_ptr().cast_mut().cast(),0,
            PROC_THREAD_ATTRIBUTE_ALL_APPLICATION_PACKAGES_POLICY as usize,
            (opt_out as *const u32).cast_mut().cast(),size_of::<u32>(),
            std::ptr::null_mut(),std::ptr::null_mut())}==0 {
            return Err(io::Error::last_os_error());
        }
        Ok(result)
    }
}
impl Drop for Attributes {
    fn drop(&mut self){
        // SAFETY: this object owns the initialized list and its backing storage.
        unsafe {DeleteProcThreadAttributeList(self.storage.as_mut_ptr().cast());}
    }
}
pub struct Child {process:OwnedHandle, thread:Option<OwnedHandle>, finished:Option<ExitStatus>}
impl AsRawHandle for Child {fn as_raw_handle(&self)->RawHandle{self.process.as_raw_handle()}}
impl Child {
    pub fn resume(&mut self)->io::Result<()> {
        let thread=self.thread.take().ok_or_else(invalid)?;
        // SAFETY: owned primary thread remains suspended; caller has verified confinement.
        let previous=unsafe {ResumeThread(thread.as_raw_handle())};
        if previous==u32::MAX {return Err(io::Error::last_os_error());}
        if previous!=1 {return Err(io::Error::other("Unexpected native suspension count"));}
        Ok(())
    }
    pub fn kill(&mut self)->io::Result<()> {
        if self.finished.is_some(){return Ok(());}
        // SAFETY: query and termination target only this newly created owned process.
        if unsafe {WaitForSingleObject(self.as_raw_handle(),0)}==WAIT_OBJECT_0 {return Ok(());}
        if unsafe {TerminateProcess(self.as_raw_handle(),1)}==0 {
            return Err(io::Error::last_os_error());
        }Ok(())
    }
    pub fn wait(&mut self)->io::Result<ExitStatus> {
        if let Some(status)=self.finished{return Ok(status);}
        // SAFETY: owned process handle stays live throughout the wait.
        let waited=unsafe {WaitForSingleObject(self.as_raw_handle(),2_000)};
        if waited==258 {
            return Err(io::Error::new(io::ErrorKind::TimedOut,"Native child reap timed out; Host Job must close"));
        }
        if waited!=WAIT_OBJECT_0 {return Err(io::Error::last_os_error());}
        let mut code=0;
        // SAFETY: owned signalled process and writable exit-status output.
        if unsafe {GetExitCodeProcess(self.as_raw_handle(),&mut code)}==0 {
            return Err(io::Error::last_os_error());
        }
        let status=ExitStatus::from_raw(code);self.finished=Some(status);Ok(status)
    }
}
impl Drop for Child {
    fn drop(&mut self){let _=self.kill();let _=self.wait();}
}
pub fn spawn(command:&Command, log:&File, parent_sid:&[u8],
    phase:&mut &'static str)->io::Result<(Child,File,File)> {
    diagnostics::preflight(command, log, phase)?;
    let application=wide(command.get_program())?;
    let directory=wide(command.get_current_dir().ok_or_else(invalid)?.as_os_str())?;
    let mut line=command_line(command)?;
    let environment=environment(command)?;
    *phase="create_stdin_pipe";
    let (child_input,parent_input)=pipe()?;
    noninheritable(&parent_input)?;
    *phase="create_stdout_pipe";
    let (parent_output,child_output)=pipe()?;
    noninheritable(&parent_output)?;
    *phase="prepare_child_handles";
    let child_error=inherited_log(log)?;
    let handles=[child_input.as_raw_handle(),child_output.as_raw_handle(),child_error.as_raw_handle()];
    // Require the exact already-attested Host identity; no derived SID or capability.
    if parent_sid.len()<8 || parent_sid[0]!=1 || parent_sid.len()!=8+usize::from(parent_sid[1])*4 {
        return Err(invalid());
    }
    let capabilities=SECURITY_CAPABILITIES{
        AppContainerSid:parent_sid.as_ptr().cast_mut().cast(),
        Capabilities:std::ptr::null_mut(),CapabilityCount:0,Reserved:0};
    let opt_out=PROCESS_CREATION_ALL_APPLICATION_PACKAGES_OPT_OUT;
    let mut attributes=Attributes::new(&handles,&capabilities,&opt_out)?;
    // SAFETY: STARTUPINFOEXW and PROCESS_INFORMATION are plain Win32 POD outputs.
    let mut si:STARTUPINFOEXW=unsafe{std::mem::zeroed()};
    si.StartupInfo.cb=size_of::<STARTUPINFOEXW>() as u32;
    si.StartupInfo.dwFlags=STARTF_USESTDHANDLES;
    si.StartupInfo.hStdInput=handles[0];si.StartupInfo.hStdOutput=handles[1];si.StartupInfo.hStdError=handles[2];
    si.lpAttributeList=attributes.storage.as_mut_ptr().cast();
    // SAFETY: zeroed Win32 output struct has no resources until CreateProcess succeeds.
    let mut pi:PROCESS_INFORMATION=unsafe{std::mem::zeroed()};
    *phase="create_blender_process";
    // SAFETY: fixed executable, checked quoting/environment, exactly three handles, no
    // alternate token/parent or breakaway; exact parent LPAC identity is explicit; Job is inherited.
    if unsafe{CreateProcessW(application.as_ptr(),line.as_mut_ptr(),std::ptr::null(),
        std::ptr::null(),1,CREATE_NO_WINDOW|CREATE_SUSPENDED|CREATE_UNICODE_ENVIRONMENT|EXTENDED_STARTUPINFO_PRESENT,
        environment.as_ptr().cast(),directory.as_ptr(),&si.StartupInfo,&mut pi)}==0 {
        let error=io::Error::last_os_error();
        diagnostics::record_create_error(log,&error);
        return Err(error);
    }
    // SAFETY: CreateProcess returned two owned process/thread handles.
    let child=unsafe{Child{process:OwnedHandle::from_raw_handle(pi.hProcess),
        thread:Some(OwnedHandle::from_raw_handle(pi.hThread)),finished:None}};
    Ok((child,parent_input,parent_output))
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn quotes_preserve_spaces_backslashes_quotes_and_empty_args(){
        for (input,expected) in [
            ("",r#""""#),("a b",r#""a b""#),(r#"a"b"#,r#""a\"b""#),
            ("C:\\x\\",r#""C:\x\\""#)
        ]{
            let mut out=Vec::new();quote(OsStr::new(input),&mut out).unwrap();
            assert_eq!(String::from_utf16(&out).unwrap(),expected);
        }
        assert!(quote(OsStr::new("x\0y"),&mut Vec::new()).is_err());
    }
    #[test]
    fn synchronous_private_pipes_preserve_binary_bytes(){
        use std::io::{Read,Write};
        let(mut read,mut write)=pipe().unwrap();
        noninheritable(&read).unwrap();noninheritable(&write).unwrap();
        write.write_all(&[0,10,13,255]).unwrap();
        let mut bytes=[0;4];read.read_exact(&mut bytes).unwrap();
        assert_eq!(bytes,[0,10,13,255]);
    }
}
