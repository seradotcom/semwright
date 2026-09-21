//! Single-threaded pre-exec Landlock helper. Never run restrictions on a Tokio worker.
use landlock::{Access,AccessFs,Compatible,CompatLevel,PathBeneath,PathFd,Ruleset,RulesetAttr,RulesetCreatedAttr,RulesetStatus,ABI};
use std::{os::unix::process::CommandExt,path::Path,process::Command};
fn main(){if run().is_err(){eprintln!("SandboxDenied: required isolation could not be established");std::process::exit(125);}}
fn run()->Result<(),Box<dyn std::error::Error>>{
    let mut args=std::env::args().skip(1);let mut writable=vec!["/tmp".to_owned()];
    loop{match args.next().as_deref(){Some("--write-root")=>{let p=args.next().ok_or("write root missing")?;if !p.starts_with("/workspace/")||p.contains("..")||p.contains('\0'){return Err("invalid sandbox root".into());}writable.push(p);},Some("--")=>break,_=>return Err("invalid sandbox arguments".into())}}
    let executable=args.next().ok_or("executable missing")?;
    if executable!="/plugin/bin"||args.next().is_some(){return Err("sandbox executable is fixed".into());}
    // SAFETY: prctl with PR_SET_NO_NEW_PRIVS takes integer options only.
    if unsafe{libc::prctl(libc::PR_SET_NO_NEW_PRIVS,1,0,0,0)}!=0{return Err("no_new_privs failed".into());}
    for(resource,limit)in [(libc::RLIMIT_NOFILE,128u64),(libc::RLIMIT_NPROC,32),(libc::RLIMIT_CPU,20),(libc::RLIMIT_AS,536_870_912),(libc::RLIMIT_FSIZE,16_777_216),(libc::RLIMIT_CORE,0)]{
        let limits=libc::rlimit{rlim_cur:limit as libc::rlim_t,rlim_max:limit as libc::rlim_t};
        // SAFETY: setrlimit synchronously reads a live repr(C) rlimit pointer.
        if unsafe{libc::setrlimit(resource,&limits)}!=0{return Err("resource limit failed".into());}
    }
    let abi=ABI::V3;let all=AccessFs::from_all(abi);let read=AccessFs::from_read(abi);
    let mut ruleset=Ruleset::default().set_compatibility(CompatLevel::HardRequirement).handle_access(all)?.create()?;
    for path in ["/usr","/lib","/lib64","/etc","/plugin","/workspace","/dev","/proc"]{
        if Path::new(path).exists(){ruleset=ruleset.add_rule(PathBeneath::new(PathFd::new(path)?,read))?;}
    }
    for path in writable{ruleset=ruleset.add_rule(PathBeneath::new(PathFd::new(path)?,all))?;}
    let status=ruleset.restrict_self()?;
    if status.ruleset!=RulesetStatus::FullyEnforced{return Err("Landlock was not fully enforced".into());}
    let error=Command::new(executable).exec();Err(Box::new(error))
}
