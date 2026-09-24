#![cfg_attr(not(windows), allow(dead_code))]

#[cfg(windows)]
mod app {
    use windows::{core::{w, Result}, Win32::{Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, WPARAM}, System::LibraryLoader::GetModuleHandleW, UI::{Controls::{InitCommonControls, WC_LISTVIEWW, WC_TREEVIEWW}, WindowsAndMessaging::*}}};

    const ID_BUTTON:isize=101; const ID_EDIT:isize=102; const ID_CHECK:isize=103; const ID_RADIO:isize=104;
    const ID_SLIDER:isize=105; const ID_COMBO:isize=106; const ID_LIST:isize=107; const ID_TREE:isize=108; const ID_TABLE:isize=109;

    unsafe extern "system" fn proc(hwnd:HWND,msg:u32,wparam:WPARAM,lparam:LPARAM)->LRESULT{
        match msg {
            WM_COMMAND => {
                if (wparam.0 & 0xffff) as isize == ID_BUTTON {
                    unsafe { SetWindowTextW(GetDlgItem(hwnd,ID_EDIT as i32),w!("invoked")); }
                }
                LRESULT(0)
            }
            WM_DESTROY => { unsafe { PostQuitMessage(0); } LRESULT(0) }
            _ => unsafe { DefWindowProcW(hwnd,msg,wparam,lparam) },
        }
    }

    unsafe fn child(parent:HWND,class:windows::core::PCWSTR,text:windows::core::PCWSTR,style:WINDOW_STYLE,x:i32,y:i32,wid:i32,hei:i32,id:isize)->HWND{
        unsafe { CreateWindowExW(WINDOW_EX_STYLE::default(),class,text,WS_CHILD|WS_VISIBLE|style,x,y,wid,hei,Some(parent),Some(HMENU(id as *mut core::ffi::c_void)),None,None) }.unwrap()
    }

    pub fn run()->Result<()> {
        unsafe { InitCommonControls(); }
        let module=unsafe{GetModuleHandleW(None)?}; let instance=HINSTANCE(module.0);
        let class=w!("SemwrightWindowsFixture");
        let wc=WNDCLASSW{hCursor:unsafe{LoadCursorW(None,IDC_ARROW)?},hInstance:instance,lpszClassName:class,lpfnWndProc:Some(proc),..Default::default()};
        unsafe { RegisterClassW(&wc); }
        let hwnd=unsafe{CreateWindowExW(WINDOW_EX_STYLE::default(),class,w!("Semwright UIA Fixture"),WS_OVERLAPPEDWINDOW|WS_VISIBLE,100,100,760,560,None,None,Some(instance),None)?};
        unsafe {
            child(hwnd,w!("BUTTON"),w!("Invoke me"),BS_PUSHBUTTON,20,20,130,32,ID_BUTTON);
            child(hwnd,w!("EDIT"),w!("fixture text"),WS_BORDER|ES_AUTOHSCROLL,170,20,240,32,ID_EDIT);
            child(hwnd,w!("BUTTON"),w!("Checked"),BS_AUTOCHECKBOX,20,70,130,28,ID_CHECK);
            child(hwnd,w!("BUTTON"),w!("Radio"),BS_AUTORADIOBUTTON,170,70,130,28,ID_RADIO);
            child(hwnd,w!("msctls_trackbar32"),w!(""),WINDOW_STYLE::default(),20,115,390,42,ID_SLIDER);
            child(hwnd,w!("COMBOBOX"),w!(""),CBS_DROPDOWNLIST|WS_VSCROLL,20,170,190,120,ID_COMBO);
            child(hwnd,w!("LISTBOX"),w!(""),WS_BORDER|LBS_NOTIFY,230,170,180,120,ID_LIST);
            child(hwnd,WC_TREEVIEWW,w!(""),WS_BORDER,20,310,190,160,ID_TREE);
            child(hwnd,WC_LISTVIEWW,w!(""),WS_BORDER,230,310,360,160,ID_TABLE);
            SendMessageW(GetDlgItem(hwnd,ID_COMBO as i32),CB_ADDSTRING,WPARAM(0),LPARAM(w!("Alpha").0 as isize));
            SendMessageW(GetDlgItem(hwnd,ID_COMBO as i32),CB_ADDSTRING,WPARAM(0),LPARAM(w!("Beta").0 as isize));
            SendMessageW(GetDlgItem(hwnd,ID_COMBO as i32),CB_SETCURSEL,WPARAM(0),LPARAM(0));
            SendMessageW(GetDlgItem(hwnd,ID_LIST as i32),LB_ADDSTRING,WPARAM(0),LPARAM(w!("List item one").0 as isize));
            SendMessageW(GetDlgItem(hwnd,ID_LIST as i32),LB_ADDSTRING,WPARAM(0),LPARAM(w!("List item two").0 as isize));
        }
        let mut msg=MSG::default(); while unsafe{GetMessageW(&mut msg,None,0,0)}.as_bool(){unsafe{TranslateMessage(&msg);DispatchMessageW(&msg);}}
        Ok(())
    }
}

#[cfg(windows)] fn main()->windows::core::Result<()> { app::run() }
#[cfg(not(windows))] fn main() { eprintln!("Windows-only fixture"); }
