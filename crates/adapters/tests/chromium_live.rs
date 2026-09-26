//! Real Rust CDP adapter against an owned, disposable browser and loopback HTTP fixture.
use futures_util::FutureExt;
use semwright_adapters::chromium::{BrowserConfig, Chromium};
use semwright_backend_api::{Backend, Context};
use semwright_types::{ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::{
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::Mutex,
};
use tokio_util::sync::CancellationToken;
type TestResult<T = ()> = Result<T, Box<dyn std::error::Error + Send + Sync>>;
async fn fixture(stop: CancellationToken, requests: Arc<Mutex<Vec<String>>>) -> TestResult<String> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let origin = format!("http://{}", listener.local_addr()?);
    tokio::spawn(async move {
        loop {
            let accepted =
                tokio::select! {_=stop.cancelled()=>break,result=listener.accept()=>result};
            let Ok((mut stream, _)) = accepted else {
                break;
            };
            let mut bytes = [0u8; 8192];
            let size =
                match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut bytes)).await {
                    Ok(Ok(size)) => size,
                    _ => continue,
                };
            let line = String::from_utf8_lossy(&bytes[..size])
                .lines()
                .next()
                .unwrap_or("")
                .to_owned();
            requests.lock().await.push(line.clone());
            let path = line.split_whitespace().nth(1).unwrap_or("/");
            if path == "/large-file" {
                let body = vec![b'x'; 128 * 1024];
                let headers = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/octet-stream\r\nContent-Disposition: attachment; filename=large.bin\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(headers.as_bytes()).await;
                let _ = stream.write_all(&body).await;
                continue;
            }
            let body = match path {
                "/frames" => {
                    "<!doctype html><title>Frames</title><div id='stable'>Stable</div><iframe src='/frame-a'></iframe>"
                }
                "/frame-a" => {
                    "<!doctype html><meta http-equiv='refresh' content='2;url=/frame-b'><p>Frame A</p>"
                }
                "/frame-b" => "<!doctype html><p>Frame B</p>",
                "/popup" => "<!doctype html><title>Semwright Popup</title><p>Popup ready</p>",
                _ => {
                    "<!doctype html><title>Semwright fixture</title><form action='/done'><label>Name<input id='name' name='name' aria-label='Name'></label><label><input id='remember' type='checkbox' aria-label='Remember me'>Remember me</label><label>Country<select id='country' aria-label='Country'><option>Mexico</option><option>Canada</option></select></label><button id='submit'>Submit</button></form><div id='editor' role='textbox' aria-label='Editor' contenteditable='true'>Draft</div><button id='dialog' onclick=\"confirm('Confirm semantic action')\">Open dialog</button><button id='late' hidden>Loaded later</button><script>setTimeout(()=>document.getElementById('late').hidden=false,150)</script><div id='shadow-host'><template shadowrootmode='open'><button id='shadow-save'>Shadow Save</button></template></div><div id='drag-source' draggable='true' aria-label='Drag source' style='width:96px;height:32px'>Drag source</div><div id='drag-target' aria-label='Drag target' style='width:96px;height:32px'>Drag target</div><a id='popup' target='_blank' href='/popup'>Open popup</a><a id='download' href='/file' download='fixture.txt'>Download</a><a id='large-download' href='/large-file' download='large.bin'>Large Download</a>"
                }
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    Ok(origin)
}

async fn cross_origin_frame_fixture(stop: CancellationToken) -> TestResult<(String, String)> {
    let child_listener = TcpListener::bind("127.0.0.2:0").await?;
    let child_origin = format!("http://{}", child_listener.local_addr()?);
    let child_stop = stop.clone();
    tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {
                _=child_stop.cancelled()=>break,
                result=child_listener.accept()=>result
            };
            let Ok((mut stream, _)) = accepted else { break };
            let mut bytes = [0u8; 4096];
            let size =
                match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut bytes)).await {
                    Ok(Ok(size)) => size,
                    _ => continue,
                };
            let request = String::from_utf8_lossy(&bytes[..size]);
            let path = request
                .lines()
                .next()
                .and_then(|line| line.split_whitespace().nth(1))
                .unwrap_or("/");
            let body = if path == "/oopif-b" {
                "<!doctype html><title>OOPIF B</title><p>Cross Frame B</p><input aria-label='Cross Name B'>"
            } else {
                "<!doctype html><title>OOPIF A</title><meta http-equiv='refresh' content='5;url=/oopif-b'><p>Cross Frame A</p><input aria-label='Cross Name A'>"
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });

    let parent_listener = TcpListener::bind("127.0.0.1:0").await?;
    let parent_origin = format!("http://{}", parent_listener.local_addr()?);
    let child_url = format!("{child_origin}/oopif-a");
    let parent_stop = stop.clone();
    tokio::spawn(async move {
        loop {
            let accepted = tokio::select! {
                _=parent_stop.cancelled()=>break,
                result=parent_listener.accept()=>result
            };
            let Ok((mut stream, _)) = accepted else { break };
            let mut bytes = [0u8; 4096];
            let size =
                match tokio::time::timeout(Duration::from_secs(2), stream.read(&mut bytes)).await {
                    Ok(Ok(size)) => size,
                    _ => continue,
                };
            if size == 0 {
                continue;
            }
            let body = format!(
                "<!doctype html><title>OOPIF Parent</title><div id='stable-parent'>Parent stable</div><iframe src='{child_url}'></iframe>"
            );
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: text/html\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            let _ = stream.write_all(response.as_bytes()).await;
        }
    });
    Ok((parent_origin, child_origin))
}

fn target(value: &Value) -> TestResult<NativeTarget> {
    Ok(serde_json::from_value(value["$ref"].clone())?)
}
async fn query(
    browser: &Chromium,
    ctx: &Context,
    tab: &NativeTarget,
    selector: &str,
) -> TestResult<NativeTarget> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = browser
                .execute(
                    ctx,
                    "browser.dom.query",
                    &json!({"_target":tab,"selector":selector}),
                )
                .await;
            if let Ok(value) = result
                && value["count"] == 1
            {
                return target(&value["matches"][0]["ref"]);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await?
}

async fn semantic_query(
    browser: &Chromium,
    ctx: &Context,
    target_ref: &NativeTarget,
    role: &str,
    name: &str,
) -> TestResult<NativeTarget> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let result = browser
                .execute(
                    ctx,
                    "browser.semantic.query",
                    &json!({
                        "_target":target_ref,
                        "role":role,
                        "name":name,
                        "exact_name":true,
                        "max_results":8,
                        "depth":16
                    }),
                )
                .await;
            if let Ok(value) = result
                && value["count"] == 1
                && !value["matches"][0]["ref"].is_null()
            {
                return target(&value["matches"][0]["ref"]);
            }
            tokio::time::sleep(Duration::from_millis(30)).await;
        }
    })
    .await?
}

async fn frame_by_origin(
    browser: &Chromium,
    ctx: &Context,
    tab: &NativeTarget,
    origin: &str,
) -> TestResult<NativeTarget> {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(value) = browser
                .execute(ctx, "browser.frame.list", &json!({"_target":tab}))
                .await
                && let Some(row) = value["frames"].as_array().and_then(|rows| {
                    rows.iter()
                        .find(|row| row["url_origin"].as_str() == Some(origin))
                })
                && row["allowed_origin"] == true
            {
                return target(&row["ref"]);
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await?
}

async fn exercise(
    browser: &Chromium,
    ctx: &Context,
    origin: &str,
    requests: &Arc<Mutex<Vec<String>>>,
) -> TestResult<PathBuf> {
    assert!(
        !browser
            .probe()
            .await
            .iter()
            .any(|f| f.capability == "browser.running" && f.usable())
    );
    browser
        .execute(ctx, "browser.launch", &json!({"headless":true}))
        .await?;
    assert!(
        browser
            .probe()
            .await
            .iter()
            .any(|f| f.capability == "browser.running" && f.usable())
    );
    let opened = browser
        .execute(
            ctx,
            "browser.tab.open",
            &json!({"url":format!("{origin}/")}),
        )
        .await?;
    let tab = target(&opened["ref"])?;
    let semantic = browser
        .execute(
            ctx,
            "browser.semantic.snapshot",
            &json!({"_target":tab,"depth":16,"max_nodes":256}),
        )
        .await?;
    assert_eq!(semantic["source"], "cdp_accessibility_tree");
    assert_eq!(semantic["arbitrary_javascript"], false);
    let semantic_nodes = semantic["nodes"]
        .as_array()
        .ok_or("semantic nodes missing")?;
    assert!(
        semantic_nodes
            .iter()
            .any(|row| row["role"] == "textbox" && row["name"] == "Name")
    );
    assert!(
        semantic_nodes
            .iter()
            .any(|row| row["role"] == "button" && row["name"] == "Shadow Save")
    );

    let editor = semantic_query(browser, ctx, &tab, "textbox", "Editor").await?;
    let inspected = browser
        .execute(ctx, "browser.semantic.inspect", &json!({"_target":editor}))
        .await?;
    assert_eq!(inspected["semantic"]["role"], "textbox");
    assert!(
        inspected["semantic"]["actions"]
            .as_array()
            .is_some_and(|actions| { actions.iter().any(|action| action == "fill") })
    );
    browser
        .execute(ctx, "browser.element.focus", &json!({"_target":editor}))
        .await?;
    browser
        .execute(ctx, "browser.element.scroll", &json!({"_target":editor}))
        .await?;
    browser
        .execute(
            ctx,
            "browser.element.fill",
            &json!({"_target":editor,"text":"Edited semantically"}),
        )
        .await?;
    assert_eq!(
        browser.validate(&editor).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let waited = browser
        .execute(
            ctx,
            "browser.semantic.wait",
            &json!({
                "_target":tab,
                "role":"button",
                "name":"Loaded later",
                "exact_name":true,
                "present":true,
                "timeout_ms":5_000,
                "poll_ms":50
            }),
        )
        .await?;
    assert_eq!(waited["satisfied"], true);
    assert_eq!(waited["present"], true);

    let late = semantic_query(browser, ctx, &tab, "button", "Loaded later").await?;
    browser
        .execute(ctx, "browser.element.hover", &json!({"_target":late}))
        .await?;
    assert_eq!(
        browser.validate(&late).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let remember = semantic_query(browser, ctx, &tab, "checkbox", "Remember me").await?;
    let checked = browser
        .execute(
            ctx,
            "browser.element.check",
            &json!({"_target":remember,"checked":true}),
        )
        .await?;
    assert_eq!(checked["changed"], true);
    assert_eq!(checked["checked"], true);
    assert_eq!(
        browser.validate(&remember).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let country = semantic_query(browser, ctx, &tab, "combobox", "Country").await?;
    let selected = browser
        .execute(
            ctx,
            "browser.element.select",
            &json!({"_target":country,"label":"Canada"}),
        )
        .await?;
    assert_eq!(selected["selected"], "Canada");
    assert_eq!(
        browser.validate(&country).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let press_target = semantic_query(browser, ctx, &tab, "textbox", "Name").await?;
    let pressed = browser
        .execute(
            ctx,
            "browser.element.press",
            &json!({"_target":press_target,"key":"Tab","modifiers":[]}),
        )
        .await?;
    assert_eq!(pressed["key"], "Tab");
    assert_eq!(
        browser.validate(&press_target).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let drag_source = query(browser, ctx, &tab, "#drag-source").await?;
    let drag_target = query(browser, ctx, &tab, "#drag-target").await?;
    let dragged = browser
        .execute(
            ctx,
            "browser.element.drag_to",
            &json!({"_target":drag_source.clone(),"_target2":drag_target.clone()}),
        )
        .await?;
    assert_eq!(dragged["accepted"], true);
    assert_eq!(dragged["same_frame"], true);
    assert_eq!(
        browser.validate(&drag_source).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    assert_eq!(
        browser.validate(&drag_target).await.unwrap_err().code,
        ErrorCode::StaleReference
    );

    let popup_link = semantic_query(browser, ctx, &tab, "link", "Open popup").await?;
    browser
        .execute(ctx, "browser.element.click", &json!({"_target":popup_link}))
        .await?;
    let popup = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let tabs = browser.execute(ctx, "browser.tab.list", &json!({})).await?;
            if let Some(row) = tabs["tabs"]
                .as_array()
                .and_then(|rows| rows.iter().find(|row| row["title"] == "Semwright Popup"))
            {
                return Ok::<NativeTarget, semwright_types::Error>(serde_json::from_value(
                    row["ref"].clone(),
                )?);
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await??;
    browser.validate(&popup).await?;
    browser
        .execute(ctx, "browser.tab.focus", &json!({"_target":popup.clone()}))
        .await?;
    browser
        .execute(ctx, "browser.tab.close", &json!({"_target":popup.clone()}))
        .await?;
    assert_eq!(
        browser.validate(&popup).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    browser.validate(&tab).await?;

    let dialog_button = semantic_query(browser, ctx, &tab, "button", "Open dialog").await?;
    browser
        .execute(
            ctx,
            "browser.element.click",
            &json!({"_target":dialog_button}),
        )
        .await?;
    let dialog = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            let status = browser
                .execute(ctx, "browser.dialog.status", &json!({"_target":tab}))
                .await?;
            if status["open"] == true {
                return Ok::<Value, semwright_types::Error>(status);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    })
    .await??;
    assert_eq!(dialog["dialog_type"], "confirm");
    assert_eq!(dialog["message"], "Confirm semantic action");
    browser
        .execute(
            ctx,
            "browser.dialog.respond",
            &json!({"_target":tab,"accept":false}),
        )
        .await?;
    let closed = browser
        .execute(ctx, "browser.dialog.status", &json!({"_target":tab}))
        .await?;
    assert_eq!(closed["open"], false);

    let input = semantic_query(browser, ctx, &tab, "textbox", "Name").await?;
    browser
        .execute(
            ctx,
            "browser.element.fill",
            &json!({"_target":input,"text":"Semwright integration"}),
        )
        .await?;
    let snapshot = browser
        .execute(
            ctx,
            "browser.dom.snapshot",
            &json!({"_target":tab,"depth":4}),
        )
        .await?;
    assert_eq!(snapshot["sensitive_attributes_omitted"], true);
    let submit = semantic_query(browser, ctx, &tab, "button", "Submit").await?;
    browser
        .execute(ctx, "browser.element.click", &json!({"_target":submit}))
        .await?;
    tokio::time::timeout(Duration::from_secs(10), async {
        while !requests
            .lock()
            .await
            .iter()
            .any(|r| r.contains("/done?name=Semwright+integration"))
        {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await?;
    assert_eq!(
        browser.validate(&input).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    let back = browser
        .execute(ctx, "browser.page.back", &json!({"_target":tab}))
        .await?;
    assert_eq!(back["changed"], true);
    let _ = semantic_query(browser, ctx, &tab, "textbox", "Name").await?;
    let forward = browser
        .execute(ctx, "browser.page.forward", &json!({"_target":tab}))
        .await?;
    assert_eq!(forward["changed"], true);
    let _ = semantic_query(browser, ctx, &tab, "textbox", "Name").await?;
    browser
        .execute(
            ctx,
            "browser.page.reload",
            &json!({"_target":tab,"ignore_cache":true}),
        )
        .await?;
    let _ = semantic_query(browser, ctx, &tab, "textbox", "Name").await?;
    browser
        .execute(ctx, "browser.page.stop", &json!({"_target":tab}))
        .await?;
    let screenshot = browser
        .execute(ctx, "browser.screenshot", &json!({"_target":tab}))
        .await?;
    let encoded = serde_json::to_string(&screenshot)?;
    assert!(
        !encoded.contains("base64"),
        "Screenshots must use artifact handles"
    );
    println!("Rust CDP screenshot artifact metadata: {screenshot}");
    let screenshot_path = PathBuf::from(
        screenshot["artifact"]["path"]
            .as_str()
            .ok_or("Screenshot artifact path missing")?,
    );
    assert!(screenshot_path.is_file());
    let download = query(browser, ctx, &tab, "#download").await?;
    browser
        .execute(ctx, "browser.dom.click", &json!({"_target":download}))
        .await?;
    let downloads = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let value = browser
                .execute(ctx, "browser.downloads.status", &json!({}))
                .await?;
            if value["events"]
                .as_array()
                .is_some_and(|events| events.iter().any(|e| e.to_string().contains("canceled")))
            {
                return Ok::<Value, semwright_types::Error>(value);
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await??;
    assert_eq!(downloads["enabled"], false);
    assert_eq!(downloads["automatic_download_size_limit"], true);
    assert!(downloads.get("download_directory").is_none());
    assert!(downloads["artifacts"].as_array().is_some_and(Vec::is_empty));
    assert_eq!(
        browser
            .execute(
                ctx,
                "browser.tab.open",
                &json!({"url":"https://not-allowed.invalid/"})
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::PolicyDenied
    );
    browser
        .execute(ctx, "browser.tab.close", &json!({"_target":tab}))
        .await?;
    // Closing is an immediate lifecycle boundary, not a race with Target.getTargets.
    for _ in 0..16 {
        assert_eq!(
            browser.validate(&tab).await.unwrap_err().code,
            ErrorCode::StaleReference
        );
    }
    Ok(screenshot_path)
}
#[tokio::test]
#[ignore = "requires SEMWRIGHT_TEST_CHROMIUM pointing to a disposable Chromium-family executable"]
async fn real_chromium_native_input_navigation_download_denial_and_cleanup() -> TestResult {
    let executable = PathBuf::from(std::env::var("SEMWRIGHT_TEST_CHROMIUM")?);
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("browser");
    let stop = CancellationToken::new();
    let guard = stop.clone().drop_guard();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let origin = fixture(stop.clone(), requests.clone()).await?;
    let browser = Chromium::new(
        BrowserConfig {
            executable,
            allowed_origins: vec![origin.clone()],
            allow_downloads: false,
            ..Default::default()
        },
        storage.clone(),
    )?;
    let ctx = Context {
        session: "real-browser-test".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    };
    let outcome = std::panic::AssertUnwindSafe(tokio::time::timeout(
        Duration::from_secs(90),
        exercise(&browser, &ctx, &origin, &requests),
    ))
    .catch_unwind()
    .await;
    browser.shutdown().await?;
    stop.cancel();
    drop(guard);
    assert!(
        !std::fs::read_dir(&storage)?
            .any(|entry| entry
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with("profile-")))
    );
    let screenshot_path = match outcome {
        Ok(result) => result??,
        Err(panic) => std::panic::resume_unwind(panic),
    };
    assert!(
        !screenshot_path.exists(),
        "browser-instance screenshot artifact must be removed on shutdown"
    );
    Ok(())
}

#[tokio::test]
#[ignore = "requires SEMWRIGHT_TEST_CHROMIUM pointing to a disposable Chromium-family executable"]
async fn real_chromium_cross_origin_oopif_semantics_are_scoped() -> TestResult {
    let executable = PathBuf::from(std::env::var("SEMWRIGHT_TEST_CHROMIUM")?);
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("browser");
    let stop = CancellationToken::new();
    let (parent_origin, child_origin) = cross_origin_frame_fixture(stop.clone()).await?;
    let browser = Chromium::new(
        BrowserConfig {
            executable,
            allowed_origins: vec![parent_origin.clone(), child_origin.clone()],
            allow_downloads: false,
            ..Default::default()
        },
        storage.clone(),
    )?;
    let ctx = Context {
        session: "oopif-browser-test".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    };
    browser
        .execute(&ctx, "browser.launch", &json!({"headless":true}))
        .await?;
    let opened = browser
        .execute(
            &ctx,
            "browser.tab.open",
            &json!({"url":format!("{parent_origin}/")}),
        )
        .await?;
    let tab = target(&opened["ref"])?;
    let parent_stable = query(&browser, &ctx, &tab, "#stable-parent").await?;
    let child_frame = frame_by_origin(&browser, &ctx, &tab, &child_origin).await?;
    let child_input =
        semantic_query(&browser, &ctx, &child_frame, "textbox", "Cross Name A").await?;
    browser
        .execute(
            &ctx,
            "browser.element.fill",
            &json!({"_target":child_input,"text":"OOPIF semantic input"}),
        )
        .await?;
    assert_eq!(
        browser.validate(&child_input).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    browser.validate(&parent_stable).await?;

    let post_fill_frame = frame_by_origin(&browser, &ctx, &tab, &child_origin).await?;
    let before_refresh = browser
        .execute(
            &ctx,
            "browser.semantic.snapshot",
            &json!({"_target":post_fill_frame,"depth":8,"max_nodes":64}),
        )
        .await?;
    assert!(
        before_refresh["nodes"]
            .as_array()
            .is_some_and(|nodes| nodes.iter().any(|row| row["name"] == "Cross Frame A"))
    );

    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            match browser.validate(&post_fill_frame).await {
                Err(error) if error.code == ErrorCode::StaleReference => break,
                _ => tokio::time::sleep(Duration::from_millis(50)).await,
            }
        }
    })
    .await?;
    browser.validate(&parent_stable).await?;

    let refreshed_frame = frame_by_origin(&browser, &ctx, &tab, &child_origin).await?;
    let refreshed = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            if let Ok(value) = browser
                .execute(
                    &ctx,
                    "browser.semantic.snapshot",
                    &json!({"_target":refreshed_frame,"depth":8,"max_nodes":64}),
                )
                .await
                && value["nodes"]
                    .as_array()
                    .is_some_and(|nodes| nodes.iter().any(|row| row["name"] == "Cross Frame B"))
            {
                break Ok::<Value, semwright_types::Error>(value);
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await??;
    assert_eq!(refreshed["source"], "cdp_accessibility_tree");
    browser.validate(&parent_stable).await?;

    browser.shutdown().await?;
    stop.cancel();
    assert!(
        !std::fs::read_dir(&storage)?
            .any(|entry| entry
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with("profile-")))
    );
    Ok(())
}

fn kill_owned_browser_processes(storage: &Path) -> TestResult<usize> {
    let marker = storage.to_string_lossy();
    let uid = std::fs::metadata("/proc/self")?.uid();
    let mut pids = Vec::new();
    for entry in std::fs::read_dir("/proc")? {
        let entry = entry?;
        let Ok(pid) = entry.file_name().to_string_lossy().parse::<u32>() else {
            continue;
        };
        let process_dir = entry.path();
        let Ok(metadata) = std::fs::metadata(&process_dir) else {
            continue;
        };
        if metadata.uid() != uid {
            continue;
        }
        let Ok(cmdline) = std::fs::read(process_dir.join("cmdline")) else {
            continue;
        };
        let text = String::from_utf8_lossy(&cmdline).replace('\0', " ");
        if text.contains(marker.as_ref()) {
            pids.push(pid);
        }
    }

    if pids.is_empty() {
        return Err("owned Chromium process was not found".into());
    }
    let mut command = Command::new("/bin/kill");
    command.arg("-KILL");
    for pid in &pids {
        command.arg(pid.to_string());
    }
    let status = command.status()?;
    if !status.success() {
        return Err("failed to kill owned Chromium process set".into());
    }
    Ok(pids.len())
}

async fn wait_until_not_running(browser: &Chromium, ctx: &Context) -> TestResult {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = browser.execute(ctx, "browser.status", &json!({})).await?;
            if status["running"] == false {
                return Ok::<(), semwright_types::Error>(());
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    })
    .await??;
    Ok(())
}

async fn stable_main_document_ref(
    browser: &Chromium,
    ctx: &Context,
    tab: &NativeTarget,
) -> TestResult<NativeTarget> {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let candidate = query(browser, ctx, tab, "#stable").await?;
            tokio::time::sleep(Duration::from_millis(100)).await;
            if browser.validate(&candidate).await.is_ok() {
                return Ok::<NativeTarget, Box<dyn std::error::Error + Send + Sync>>(candidate);
            }
        }
    })
    .await?
}

#[tokio::test]
#[ignore = "requires SEMWRIGHT_TEST_CHROMIUM pointing to a disposable Chromium-family executable"]
async fn real_chromium_quota_multiframe_crash_recovery_and_artifact_lifecycle() -> TestResult {
    let executable = PathBuf::from(std::env::var("SEMWRIGHT_TEST_CHROMIUM")?);
    let directory = tempfile::tempdir()?;
    let storage = directory.path().join("browser");
    let stop = CancellationToken::new();
    let requests = Arc::new(Mutex::new(Vec::new()));

    let origin = fixture(stop.clone(), requests).await?;
    let browser = Chromium::new(
        BrowserConfig {
            executable,
            allowed_origins: vec![origin.clone()],
            allow_downloads: true,
            max_download_bytes: 4 * 1024,
            max_total_download_bytes: 8 * 1024,
            max_downloads: 2,
        },
        storage.clone(),
    )?;
    let ctx = Context {
        session: "hardening-browser-test".into(),
        request_id: semwright_types::unique_id(),
        cancellation: CancellationToken::new(),
    };
    browser
        .execute(&ctx, "browser.launch", &json!({"headless":true}))
        .await?;

    let frames = browser
        .execute(
            &ctx,
            "browser.tab.open",
            &json!({"url":format!("{origin}/frames")}),
        )
        .await?;

    let frames_tab = target(&frames["ref"])?;
    let frame_list = browser
        .execute(&ctx, "browser.frame.list", &json!({"_target":frames_tab}))
        .await?;
    assert!(frame_list["count"].as_u64().is_some_and(|count| count >= 2));
    let child = frame_list["frames"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| !row["parent_frame_id"].is_null()))
        .ok_or("child frame missing")?;
    let child_frame = target(&child["ref"])?;
    let child_semantic = browser
        .execute(
            &ctx,
            "browser.semantic.snapshot",
            &json!({"_target":child_frame,"depth":8,"max_nodes":64}),
        )
        .await?;
    assert!(
        child_semantic["nodes"]
            .as_array()
            .is_some_and(|nodes| { nodes.iter().any(|row| row["name"] == "Frame A") })
    );
    let stable = stable_main_document_ref(&browser, &ctx, &frames_tab).await?;
    tokio::time::sleep(Duration::from_millis(2400)).await;
    assert_eq!(
        browser.validate(&child_frame).await.unwrap_err().code,
        ErrorCode::StaleReference,
        "subframe navigation must retire frame references"
    );
    assert_eq!(
        browser.validate(&stable).await.unwrap_err().code,
        ErrorCode::StaleReference,
        "subframe navigation must retire DOM references for the attached target"
    );
    let refreshed = query(&browser, &ctx, &frames_tab, "#stable").await?;
    browser.validate(&refreshed).await?;
    let refreshed_frames = browser
        .execute(&ctx, "browser.frame.list", &json!({"_target":frames_tab}))
        .await?;
    let refreshed_child = refreshed_frames["frames"]
        .as_array()
        .and_then(|rows| rows.iter().find(|row| !row["parent_frame_id"].is_null()))
        .ok_or("refreshed child frame missing")?;
    let refreshed_frame = target(&refreshed_child["ref"])?;
    let refreshed_semantic = browser
        .execute(
            &ctx,
            "browser.semantic.snapshot",
            &json!({"_target":refreshed_frame,"depth":8,"max_nodes":64}),
        )
        .await?;
    assert!(
        refreshed_semantic["nodes"]
            .as_array()
            .is_some_and(|nodes| { nodes.iter().any(|row| row["name"] == "Frame B") })
    );

    let page = browser
        .execute(
            &ctx,
            "browser.tab.open",
            &json!({"url":format!("{origin}/")}),
        )
        .await?;
    let page_tab = target(&page["ref"])?;
    let screenshot = browser
        .execute(&ctx, "browser.screenshot", &json!({"_target":page_tab}))
        .await?;
    let screenshot_path = PathBuf::from(
        screenshot["artifact"]["path"]
            .as_str()
            .ok_or("screenshot artifact path missing")?,
    );
    assert!(screenshot_path.is_file());

    let large = query(&browser, &ctx, &page_tab, "#large-download").await?;
    browser
        .execute(&ctx, "browser.dom.click", &json!({"_target":large}))
        .await?;
    let quota = tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let status = browser
                .execute(&ctx, "browser.downloads.status", &json!({}))
                .await?;
            if status["quota_cancellations"].as_u64().unwrap_or(0) > 0 {
                return Ok::<Value, semwright_types::Error>(status);
            }
            tokio::time::sleep(Duration::from_millis(40)).await;
        }
    })
    .await??;
    assert_eq!(quota["automatic_download_size_limit"], true);
    assert!(quota["artifacts"].as_array().is_some_and(Vec::is_empty));
    assert!(
        quota["events"]
            .as_array()
            .is_some_and(|events| { events.iter().any(|event| event["quota_exceeded"] == true) })
    );

    assert!(kill_owned_browser_processes(&storage)? > 0);
    wait_until_not_running(&browser, &ctx).await?;
    browser
        .execute(&ctx, "browser.launch", &json!({"headless":true}))
        .await?;
    assert_eq!(
        browser.validate(&page_tab).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    assert!(
        !screenshot_path.exists(),
        "crash recovery must remove browser-instance screenshot artifacts"
    );

    browser.shutdown().await?;
    stop.cancel();
    assert!(
        !std::fs::read_dir(&storage)?
            .any(|entry| entry
                .is_ok_and(|e| e.file_name().to_string_lossy().starts_with("profile-")))
    );
    Ok(())
}
