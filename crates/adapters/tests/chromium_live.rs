//! Real Rust CDP adapter against an owned, disposable browser and loopback HTTP fixture.
use futures_util::FutureExt;
use semwright_adapters::chromium::{BrowserConfig, Chromium};
use semwright_backend_api::{Backend, Context};
use semwright_types::{ErrorCode, NativeTarget};
use serde_json::{Value, json};
use std::{path::PathBuf, sync::Arc, time::Duration};
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
            requests.lock().await.push(line);
            let body = "<!doctype html><title>Semwright fixture</title><form action='/done'><label>Name<input id='name' name='name' aria-label='Name'></label><button id='submit'>Submit</button></form><a id='download' href='/file' download='fixture.txt'>Download</a>";
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
) -> TestResult {
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
    let directory = downloads["download_directory"]
        .as_str()
        .ok_or("Download directory missing")?;
    assert_eq!(std::fs::read_dir(directory)?.count(), 0);
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
    assert_eq!(
        browser.validate(&tab).await.unwrap_err().code,
        ErrorCode::StaleReference
    );
    Ok(())
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
        },
        storage.clone(),
    )?;
    let ctx = Context {
        session: "real-browser-test".into(),
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
    match outcome {
        Ok(result) => result?,
        Err(panic) => std::panic::resume_unwind(panic),
    }
}
