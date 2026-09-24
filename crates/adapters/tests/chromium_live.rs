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
                _ => {
                    "<!doctype html><title>Semwright fixture</title><form action='/done'><label>Name<input id='name' name='name' aria-label='Name'></label><button id='submit'>Submit</button></form><a id='download' href='/file' download='fixture.txt'>Download</a><a id='large-download' href='/large-file' download='large.bin'>Large Download</a>"
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
    let input = query(browser, ctx, &tab, "#name").await?;
    browser
        .execute(
            ctx,
            "browser.dom.fill",
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
    let submit = query(browser, ctx, &tab, "#submit").await?;
    browser
        .execute(ctx, "browser.dom.click", &json!({"_target":submit}))
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
    let stable = stable_main_document_ref(&browser, &ctx, &frames_tab).await?;
    tokio::time::sleep(Duration::from_millis(2400)).await;
    assert_eq!(
        browser.validate(&stable).await.unwrap_err().code,
        ErrorCode::StaleReference,
        "subframe navigation must retire DOM references for the attached target"
    );
    let refreshed = query(&browser, &ctx, &frames_tab, "#stable").await?;
    browser.validate(&refreshed).await?;

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
