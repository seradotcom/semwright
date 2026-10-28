//! Only compiled for the explicit Windows launch test fixture feature.
use super::*;
#[path = "windows_activation_readback.rs"]
mod activation_readback;
#[path = "windows_cpu_fixture.rs"]
mod cpu_fixture;
#[path = "windows_cpu_images.rs"]
mod cpu_images;
#[path = "windows_cpu_payload.rs"]
mod cpu_payload;
#[path = "windows_cpu_resource_activation.rs"]
mod cpu_resource_activation;
pub fn run() -> io::Result<()> {
    // Refuse direct owner execution before any fixture filesystem access.
    let parent = verify_process(unsafe { GetCurrentProcess() })?;
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.len() == 3 && args[0] == "--activation-context-probe" {
        return activation_context_probe(Path::new(&args[1]), Path::new(&args[2]));
    }
    if args.len() == 2 && args[0] == "--embedded-cpu-actctx-probe" {
        return embedded_activation_probe(&args[1], true, true, true, false, true);
    }
    if args.len() == 2 && args[0] == "--embedded-cpu-component-probe" {
        return embedded_activation_probe(&args[1], true, true, true, true, false);
    }
    if args.len() == 2 && args[0] == "--embedded-blender-manifest-probe" {
        return embedded_activation_probe(&args[1], true, true, true, false, false);
    }
    if args.len() == 2 && args[0] == "--embedded-private-assemblies-probe" {
        return embedded_activation_probe(&args[1], true, true, false, false, false);
    }
    if args.len() == 2
        && [
            "--embedded-activation-probe",
            "--embedded-common-controls-probe",
        ]
        .contains(&args[0].as_str())
    {
        return embedded_activation_probe(
            &args[1],
            args[0] == "--embedded-common-controls-probe",
            false,
            false,
            false,
            false,
        );
    }
    if args == ["--exit-probe"] {
        return Ok(());
    }
    if args == ["--child-probe"] {
        io::stdout().write_all(b"fixture-child-lpac")?;
        return Ok(());
    }
    if args.len() != 3 || !["--launch-probe", "--launch-no-stdio-probe"].contains(&args[0].as_str())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Closed fixture arguments required",
        ));
    }
    let no_stdio = args[0] == "--launch-no-stdio-probe";
    let executable = PathBuf::from(&args[1]);
    let workspace = PathBuf::from(&args[2]);
    kind(&executable, false)?;
    kind(&workspace, true)?;
    let log = OpenOptions::new()
        .read(true)
        .write(true)
        .create_new(true)
        .open(workspace.join("launch.log"))?;
    let system = std::env::var_os("SystemRoot").ok_or_else(|| denied("Missing Host SystemRoot"))?;
    let mut command = Command::new(executable);
    command
        .arg(if no_stdio {
            "--exit-probe"
        } else {
            "--child-probe"
        })
        .current_dir(&workspace)
        .env_clear()
        .env("SystemRoot", &system)
        .env("WINDIR", &system)
        .env("TEMP", &workspace)
        .env("TMP", &workspace);
    let mut phase = "fixture_start";
    let result = (|| -> io::Result<()> {
        let (mut child, input, mut output) = if no_stdio {
            native_child::spawn_without_stdio_for_test(
                &command,
                &log,
                &parent.container,
                &mut phase,
            )?
        } else {
            native_child::spawn(&command, &log, &parent.container, &mut phase)?
        };
        phase = "fixture_child_token";
        if verify_process(child.as_raw_handle())? != parent {
            return Err(denied("Fixture child identity differs"));
        }
        phase = "fixture_child_job";
        native_child::verify_same_job(&child)?;
        phase = "fixture_resume";
        child.resume()?;
        drop(input);
        if !no_stdio {
            let mut bytes = Vec::new();
            Read::by_ref(&mut output)
                .take(8193)
                .read_to_end(&mut bytes)?;
            if bytes != b"fixture-child-lpac" {
                return Err(io::Error::other("Unexpected fixture child bytes"));
            }
        }
        let exit = child.wait()?;
        if !exit.success() {
            return Err(io::Error::other("Fixture child did not exit successfully"));
        }
        Ok(())
    })();
    let mut text = String::new();
    let mut reader = log.try_clone()?;
    reader.seek(SeekFrom::Start(0))?;
    reader.take(8192).read_to_string(&mut text)?;
    let row = serde_json::json!({"fixture":true,"native_blender":false,"ok":result.is_ok(),
        "phase":phase,"os_error":result.as_ref().err().and_then(io::Error::raw_os_error),
        "diagnostics":text});
    println!("{row}");
    result
}

const MINIMAL_ACTCTX_XML:&[u8]=br#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
<assemblyIdentity type="win32" name="Semwright.ReadOnly.ActCtxProbe" version="1.0.0.0" processorArchitecture="amd64"/>
</assembly>
"#;
fn full_manifest_probe(path: &Path) -> serde_json::Value {
    use sha2::{Digest, Sha256};
    let result = (|| -> io::Result<Vec<u8>> {
        kind(path, false)?;
        let mut bytes = Vec::new();
        File::open(path)?.take(65537).read_to_end(&mut bytes)?;
        if bytes.len() > 65536 {
            return Err(denied("Manifest diagnostic size bound exceeded"));
        }
        Ok(bytes)
    })();
    match result {
        Ok(bytes) => {
            serde_json::json!({"ok":true,"bytes":bytes.len(),"sha256":hex::encode(Sha256::digest(&bytes))})
        }
        Err(error) => {
            serde_json::json!({"ok":false,"error":error.raw_os_error(),"kind":format!("{:?}",error.kind())})
        }
    }
}
fn directory_probe(path: &Path, manifest: &str) -> serde_json::Value {
    let result = (|| -> io::Result<(usize, bool, bool)> {
        kind(path, true)?;
        let mut count = 0;
        let mut found = false;
        for entry in std::fs::read_dir(path)?.take(129) {
            let entry = entry?;
            count += 1;
            found |= entry
                .file_name()
                .to_string_lossy()
                .eq_ignore_ascii_case(manifest);
        }
        Ok((count.min(128), found, count > 128))
    })();
    match result {
        Ok((count, found, truncated)) => {
            serde_json::json!({"ok":true,"visited":count,"expected_manifest_found":found,"truncated":truncated})
        }
        Err(error) => {
            serde_json::json!({"ok":false,"error":error.raw_os_error(),"kind":format!("{:?}",error.kind())})
        }
    }
}
fn context_probe(path: &Path, embedded: bool, language: Option<u16>) -> serde_json::Value {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::System::WindowsProgramming::ACTCTX_FLAG_RESOURCE_NAME_VALID;
    use windows_sys::Win32::{
        Foundation::INVALID_HANDLE_VALUE,
        System::ApplicationInstallationAndServicing::{ACTCTXW, CreateActCtxW, ReleaseActCtx},
    };
    let source: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    // SAFETY: all optional fields stay null, buffers live through the call.
    let mut context: ACTCTXW = unsafe { std::mem::zeroed() };
    context.cbSize = std::mem::size_of::<ACTCTXW>() as u32;
    context.lpSource = source.as_ptr();
    if embedded {
        context.dwFlags = ACTCTX_FLAG_RESOURCE_NAME_VALID;
        context.lpResourceName = 1usize as *const u16;
    }
    if let Some(language) = language {
        context.dwFlags |= windows_sys::Win32::System::WindowsProgramming::ACTCTX_FLAG_LANGID_VALID;
        context.wLangId = language;
    }
    // Resolve only. Never activate the context or execute the application.
    let handle = unsafe { CreateActCtxW(&context) };
    let error = if handle == INVALID_HANDLE_VALUE {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    if error.is_none() {
        unsafe { ReleaseActCtx(handle) };
    }
    serde_json::json!({"ok":error.is_none(),"embedded_resource":embedded,"explicit_language":language,
        "win32_error":error.as_ref().and_then(io::Error::raw_os_error)})
}
fn activation_context_probe(runtime: &Path, fixture: &Path) -> io::Result<()> {
    use sha2::{Digest, Sha256};
    kind(runtime, true)?;
    kind(fixture, true)?;
    let executable = runtime.join("blender.exe");
    kind(&executable, false)?;
    let bytes = std::fs::read(&executable)?;
    let digest = hex::encode(Sha256::digest(&bytes));
    drop(bytes);
    if digest != "284f4041f98e113f3dc10654a7193ffaaa9bfdfec8b87fa116620a48b5f6d4cb" {
        return Err(denied("Activation probe exact pinned application required"));
    }
    let minimal = fixture.join("minimal.manifest");
    kind(&minimal, false)?;
    let mut fixed = Vec::new();
    File::open(&minimal)?.take(65537).read_to_end(&mut fixed)?;
    if fixed != MINIMAL_ACTCTX_XML {
        return Err(denied("Exact fixed read-only diagnostic manifest required"));
    }
    let shared = full_manifest_probe(&runtime.join("blender.shared/blender.shared.manifest"));
    let crt = full_manifest_probe(&runtime.join("blender.crt/blender.crt.manifest"));
    let shared_directory =
        directory_probe(&runtime.join("blender.shared"), "blender.shared.manifest");
    let crt_directory = directory_probe(&runtime.join("blender.crt"), "blender.crt.manifest");
    let minimal_manifest = full_manifest_probe(&minimal);
    let minimal_context = context_probe(&minimal, false, None);
    let languages = language_probe();
    let after_language_query = context_probe(&minimal, false, None);
    let explicit_language = context_probe(&minimal, false, Some(0x0409));
    let blender_context = context_probe(&executable, true, None);
    let ok = minimal_context["ok"] == true && blender_context["ok"] == true;
    println!(
        "{}",
        serde_json::json!({"probe":"closed_readonly_SxS_comparison","native_blender":false,
        "broker_used":false,"context":"verified_LPAC_no_capabilities_in_Job","image_sha256":digest,
        "shared_manifest_full":shared,"crt_manifest_full":crt,"shared_directory":shared_directory,
        "crt_directory":crt_directory,"minimal_manifest_full":minimal_manifest,
        "minimal_activation_context":minimal_context,"language_query":languages,"default_after_language_query":after_language_query,"minimal_explicit_0409":explicit_language,"blender_activation_context":blender_context,"ok":ok})
    );
    if ok {
        Ok(())
    } else {
        Err(denied("Readonly activation resolution remains blocked"))
    }
}

fn language_probe() -> serde_json::Value {
    use windows_sys::Win32::Globalization::{
        GetUserDefaultLocaleName, GetUserPreferredUILanguages, MUI_LANGUAGE_NAME,
    };
    let mut count = 0u32;
    let mut length = 0u32;
    // SAFETY: size-only query uses writable counters and a null optional output buffer.
    let sized = unsafe {
        GetUserPreferredUILanguages(
            MUI_LANGUAGE_NAME,
            &mut count,
            std::ptr::null_mut(),
            &mut length,
        )
    };
    let size_error = if sized == 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    let ui = if let Some(error) = size_error {
        serde_json::json!({"ok":false,"phase":"size","error":error.raw_os_error()})
    } else if length == 0 || length > 4096 || count > 32 {
        serde_json::json!({"ok":false,"phase":"bound","length":length,"count":count})
    } else {
        let mut buffer = vec![0u16; length as usize];
        // SAFETY: initialized UTF16 allocation has exactly the reported bounded length.
        let read = unsafe {
            GetUserPreferredUILanguages(
                MUI_LANGUAGE_NAME,
                &mut count,
                buffer.as_mut_ptr(),
                &mut length,
            )
        };
        let error = if read == 0 {
            Some(io::Error::last_os_error())
        } else {
            None
        };
        if let Some(error) = error {
            serde_json::json!({"ok":false,"phase":"read","error":error.raw_os_error()})
        } else if length as usize > buffer.len() || count > 32 {
            serde_json::json!({"ok":false,"phase":"returned_bound"})
        } else {
            let names: Vec<String> = buffer[..length as usize]
                .split(|v| *v == 0)
                .filter(|v| !v.is_empty())
                .take(32)
                .map(String::from_utf16_lossy)
                .collect();
            serde_json::json!({"ok":true,"count":count,"names":names})
        }
    };
    let mut name = [0u16; 85];
    // SAFETY: fixed buffer is LOCALE_NAME_MAX_LENGTH wide characters.
    let n = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    let error = if n == 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    let locale = if let Some(error) = error {
        serde_json::json!({"ok":false,"error":error.raw_os_error()})
    } else if n < 1 || n > 85 {
        serde_json::json!({"ok":false,"phase":"returned_bound"})
    } else {
        serde_json::json!({"ok":true,"name":String::from_utf16_lossy(&name[..n as usize-1])})
    };
    serde_json::json!({"user_ui_languages":ui,"user_locale":locale,"setting_changed":false})
}

fn embedded_activation_probe(
    nonce: &str,
    common_controls: bool,
    private_assemblies: bool,
    full_blender_manifest: bool,
    cpu_component: bool,
    cpu_actctx: bool,
) -> io::Result<()> {
    use sha2::Digest;
    use windows_sys::Win32::System::{
        LibraryLoader::*, WindowsProgramming::QUERY_ACTCTX_FLAG_USE_ACTIVE_ACTCTX,
    };
    if nonce.len() != 32 || !nonce.bytes().all(|c| c.is_ascii_hexdigit()) {
        return Err(denied("Closed nonce format required"));
    }
    // SAFETY: querying borrowed own module and its fixed integer resource; no module execution.
    let module = unsafe { GetModuleHandleW(std::ptr::null()) };
    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    let resource =
        unsafe { FindResourceExW(module, 24usize as *const u16, 1usize as *const u16, 0) };
    if resource.is_null() {
        return Err(io::Error::last_os_error());
    }
    let expected_manifest = if full_blender_manifest {
        activation_readback::fixture_blender_manifest()
    } else if private_assemblies {
        activation_readback::fixture_private_manifest()
    } else {
        activation_readback::fixture_manifest(common_controls)
    };
    let size = unsafe { SizeofResource(module, resource) } as usize;
    if size != expected_manifest.len() {
        return Err(denied("Unexpected embedded fixture size"));
    }
    let loaded = unsafe { LoadResource(module, resource) };
    if loaded.is_null() {
        return Err(io::Error::last_os_error());
    }
    let data = unsafe { LockResource(loaded) };
    if data.is_null() {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: Windows reports the fixed expected resource length, borrowed until process exit.
    let bytes = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), size) };
    if bytes != expected_manifest.as_slice() {
        return Err(denied("Embedded fixture bytes differ"));
    }
    // No dynamic ActivateActCtx/CreateActCtx has run before this initial readback. Query the active
    // context and compare its root identity to the owner control in the integration test.
    let context = activation_readback::read_context(
        QUERY_ACTCTX_FLAG_USE_ACTIVE_ACTCTX,
        std::ptr::null_mut(),
    );
    let dll = if common_controls && !cpu_actctx && context.is_ok() {
        println!("fixture_phase: fixed_comctl32_initialization");
        io::stdout().flush()?;
        Some(probe_common_controls_dll())
    } else {
        None
    };
    let cpu = if cpu_component && context.is_ok() && dll.as_ref().is_none_or(|v| v.is_ok()) {
        Some(cpu_fixture::probe())
    } else {
        None
    };
    let cpu_activation = if cpu_actctx && context.is_ok() {
        let executable = std::env::current_exe()?;
        let root = executable
            .parent()
            .ok_or_else(|| denied("Missing fixture root"))?;
        Some(cpu_resource_activation::probe(root))
    } else {
        None
    };
    let cpu_activation_ok = !cpu_actctx || cpu_activation.as_ref().is_some_and(|v| v["ok"] == true);
    let cpu_ok = !cpu_component || cpu.as_ref().is_some_and(|v| v["ok"] == true);
    let ok =
        context.is_ok() && dll.as_ref().is_none_or(|v| v.is_ok()) && cpu_ok && cpu_activation_ok;
    println!(
        "{}",
        serde_json::json!({"probe":"initial_embedded_context","native_blender":false,"broker_used":false,
        "reached_main":true,"verified_lpac":true,"capability_count":0,"job_verified":true,
        "nonce":nonce,"manifest_sha256":hex::encode(sha2::Sha256::digest(bytes)),
        "activation":context.as_ref().ok(),"query_error":context.as_ref().err().and_then(io::Error::raw_os_error),
        "common_controls":common_controls,"private_assemblies":private_assemblies,"full_blender_manifest":full_blender_manifest,"cpu_component_probe":cpu_component,
        "blender_executable_started":false,"blender_component_execution_attempted":cpu.as_ref().is_some_and(|v|v["blender_component_execution_attempted"]==true),
        "cpu_actctx_probe":cpu_actctx,"cpu_resource_activation":cpu_activation.as_ref(),
        "cpu_component":cpu.as_ref(),"dll":dll.as_ref().and_then(|v|v.as_ref().ok()),
        "dll_error":dll.as_ref().and_then(|v|v.as_ref().err()).and_then(io::Error::raw_os_error),"ok":ok})
    );
    context?;
    if let Some(result) = dll {
        result?;
    }
    if !cpu_activation_ok {
        return Err(denied("Pinned CPU activation-context diagnostic failed"));
    }
    if !cpu_ok {
        return Err(denied("Pinned CPU component diagnostic failed"));
    }
    Ok(())
}

fn probe_common_controls_dll() -> io::Result<serde_json::Value> {
    use windows_sys::Win32::{Foundation::FreeLibrary, System::LibraryLoader::*};
    let name: Vec<u16> = "comctl32.dll".encode_utf16().chain(Some(0)).collect();
    // SAFETY: fixed system DLL, explicit System32 search plus the declared SxS redirection.
    // This executes DLL initialization inside the already verified LPAC/Job.
    let module = unsafe {
        LoadLibraryExW(
            name.as_ptr(),
            std::ptr::null_mut(),
            LOAD_LIBRARY_SEARCH_SYSTEM32,
        )
    };
    if module.is_null() {
        return Err(io::Error::last_os_error());
    }
    let result = (|| -> io::Result<serde_json::Value> {
        let mut path = vec![0u16; 32768];
        let n =
            unsafe { GetModuleFileNameW(module, path.as_mut_ptr(), path.len() as u32) } as usize;
        if n == 0 {
            return Err(io::Error::last_os_error());
        }
        if n >= path.len() {
            return Err(io::Error::other("Module path truncated"));
        }
        let path =
            String::from_utf16(&path[..n]).map_err(|_| io::Error::other("Invalid module path"))?;
        if !Path::new(&path)
            .file_name()
            .is_some_and(|v| v.to_string_lossy().eq_ignore_ascii_case("comctl32.dll"))
        {
            return Err(denied("Unexpected fixed system module"));
        }
        Ok(
            serde_json::json!({"loaded":true,"module_path":path,"search_flags":LOAD_LIBRARY_SEARCH_SYSTEM32,"export_called":false}),
        )
    })();
    let released = unsafe { FreeLibrary(module) };
    let release_error = if released == 0 {
        Some(io::Error::last_os_error())
    } else {
        None
    };
    let value = result?;
    if let Some(error) = release_error {
        return Err(error);
    }
    Ok(value)
}
