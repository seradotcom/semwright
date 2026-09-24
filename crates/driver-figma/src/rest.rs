use futures_util::StreamExt;
use reqwest::{
    Client, Method, Url,
    header::{AUTHORIZATION, HeaderValue},
    redirect::Policy,
};
use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Map, Value, json};
use std::{path::Path, time::Duration};
use zeroize::Zeroizing;

#[cfg(unix)]
use std::os::unix::fs::{FileTypeExt, MetadataExt};
#[cfg(unix)]
use tokio::io::AsyncReadExt;

pub const CREDENTIAL_DIR: &str = "/workspace/figma-credential";
pub const CREDENTIAL_SOCKET: &str = "/workspace/figma-credential/secret.sock";
const API_BASE: &str = "https://api.figma.com/";
const MAX_SECRET_BYTES: usize = 4096;
const MAX_RESPONSE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AuthKind {
    OAuth,
    Personal,
    Plan,
}
struct Credential {
    kind: AuthKind,
    token: Zeroizing<Vec<u8>>,
}

#[derive(Clone)]
pub struct RestClient {
    client: Client,
    base: Url,
}

impl RestClient {
    pub fn new() -> Result<Self> {
        Self::with_base(API_BASE)
    }

    fn with_base(base: &str) -> Result<Self> {
        let base = Url::parse(base).map_err(|_| Error::invalid("Invalid Figma REST base URL"))?;
        if !matches!(base.scheme(), "https" | "http") || base.cannot_be_a_base() {
            return Err(Error::invalid("Invalid Figma REST base URL"));
        }
        let client = Client::builder()
            .redirect(Policy::none())
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(15))
            .user_agent("semwright-figma-driver")
            .build()
            .map_err(|_| {
                Error::new(
                    ErrorCode::BackendFailed,
                    "Could not initialize Figma REST client",
                )
            })?;
        Ok(Self { client, base })
    }
    #[cfg(any(test, feature = "test-tools"))]
    pub fn with_base_for_tests(base: &str) -> Result<Self> {
        Self::with_base(base)
    }

    pub fn status(&self) -> Value {
        json!({
            "configured": credential_socket_present(),
            "credential_transport": "protected_local_socket",
            "api_origin": self.base.origin().ascii_serialization(),
            "token_exposed_to_agent": false
        })
    }

    pub async fn execute(&self, operation: &str, args: &Map<String, Value>) -> Result<Value> {
        if operation == "cloud.status" {
            return Ok(self.status());
        }
        let credential = read_credential().await?;
        self.execute_authenticated(operation, args, &credential)
            .await
    }

    #[cfg(any(test, feature = "test-tools"))]
    pub async fn execute_with_test_token(
        &self,
        operation: &str,
        args: &Map<String, Value>,
        kind: &str,
        token: &[u8],
    ) -> Result<Value> {
        let credential = Credential {
            kind: match kind {
                "oauth" => AuthKind::OAuth,
                "pat" => AuthKind::Personal,
                "plan" => AuthKind::Plan,
                _ => return Err(Error::invalid("Unknown test credential kind")),
            },
            token: Zeroizing::new(token.to_vec()),
        };
        self.execute_authenticated(operation, args, &credential)
            .await
    }

    async fn execute_authenticated(
        &self,
        operation: &str,
        args: &Map<String, Value>,
        credential: &Credential,
    ) -> Result<Value> {
        let meta = crate::rest_catalog::metadata(operation)
            .ok_or_else(|| Error::new(ErrorCode::Unsupported, "Unknown Figma REST capability"))?;
        if !credential_allowed(meta.operation_id, credential.kind) {
            return Err(Error::new(
                ErrorCode::PermissionDenied,
                "Configured Figma credential type cannot call this endpoint",
            ));
        }
        let data = match operation {
            "cloud.discovery.text_events" => self.discovery(args, credential).await?,
            "cloud.file.get" => self.file_get(args, credential).await?,
            "cloud.file.nodes" => self.file_nodes(args, credential).await?,
            "cloud.images.render" => self.images_render(args, credential).await?,
            "cloud.image_fills.list" => {
                self.get(
                    &["v1", "files", required(args, "fileKey")?, "images"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.file.metadata" => self.file_metadata(args, credential).await?,
            "cloud.versions.list" => self.versions(args, credential).await?,
            "cloud.comments.list" => self.comments(args, credential).await?,
            "cloud.comment.create" => self.comment_create(args, credential).await?,
            "cloud.comment.delete" => self.comment_delete(args, credential).await?,
            "cloud.reactions.list" => self.reactions(args, credential).await?,
            "cloud.reaction.add" => self.reaction_add(args, credential).await?,
            "cloud.reaction.delete" => self.reaction_delete(args, credential).await?,
            "cloud.user.current" => self.get(&["v1", "me"], &[], credential).await?,
            "cloud.team.components" => self.team_asset(args, credential, "components").await?,
            "cloud.file.components" => self.file_asset(args, credential, "components").await?,
            "cloud.library.component.get" => {
                self.get(
                    &["v1", "components", required(args, "key")?],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.team.component_sets" => {
                self.team_asset(args, credential, "component_sets").await?
            }
            "cloud.file.component_sets" => {
                self.file_asset(args, credential, "component_sets").await?
            }
            "cloud.library.component_set.get" => {
                self.get(
                    &["v1", "component_sets", required(args, "key")?],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.team.styles" => self.team_asset(args, credential, "styles").await?,
            "cloud.file.styles" => self.file_asset(args, credential, "styles").await?,
            "cloud.library.style.get" => {
                self.get(&["v1", "styles", required(args, "key")?], &[], credential)
                    .await?
            }
            "cloud.team.folders" => self.team_folders(args, credential).await?,
            "cloud.folder.subfolders" => {
                self.get(
                    &["v2", "folders", required(args, "folderId")?, "folders"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.folder.files" => self.folder_files(args, credential).await?,
            "cloud.folder.metadata" => {
                self.get(
                    &["v2", "folders", required(args, "folderId")?, "meta"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.legacy.team.projects" => {
                self.get(
                    &["v1", "teams", required(args, "teamId")?, "projects"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.legacy.project.metadata" => {
                self.get(
                    &["v1", "projects", required(args, "projectId")?, "meta"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.legacy.project.files" => self.project_files(args, credential).await?,
            "cloud.webhooks.list" => self.webhooks_list(args, credential).await?,
            "cloud.webhook.get" => self.webhook_get(args, credential).await?,
            "cloud.webhook.create" => self.webhook_create(args, credential).await?,
            "cloud.webhook.update" => self.webhook_update(args, credential).await?,
            "cloud.webhook.delete" => self.webhook_delete(args, credential).await?,
            "cloud.legacy.team.webhooks" => {
                self.get(
                    &["v2", "teams", required(args, "teamId")?, "webhooks"],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.webhook.requests" => self.webhook_requests(args, credential).await?,
            "cloud.activity_logs.list" => self.activity_logs(args, credential).await?,
            "cloud.developer_logs.query" => self.developer_logs(args, credential).await?,
            "cloud.ai_usage.daily" => self.ai_usage(args, credential).await?,
            "cloud.payments.lookup" => self.payments(args, credential).await?,
            "cloud.variables.local" => {
                self.get(
                    &[
                        "v1",
                        "files",
                        required(args, "fileKey")?,
                        "variables",
                        "local",
                    ],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.variables.published" => {
                self.get(
                    &[
                        "v1",
                        "files",
                        required(args, "fileKey")?,
                        "variables",
                        "published",
                    ],
                    &[],
                    credential,
                )
                .await?
            }
            "cloud.variables.mutate" => self.variables_mutate(args, credential).await?,
            "cloud.dev_resources.list" => self.dev_resources(args, credential).await?,
            "cloud.dev_resources.create" => self.dev_resources_create(args, credential).await?,
            "cloud.dev_resources.update" => self.dev_resources_update(args, credential).await?,
            "cloud.dev_resource.delete" => self.dev_resource_delete(args, credential).await?,
            "cloud.library_analytics.component.actions" => {
                self.analytics(args, credential, "component", "actions")
                    .await?
            }
            "cloud.library_analytics.component.usages" => {
                self.analytics(args, credential, "component", "usages")
                    .await?
            }
            "cloud.library_analytics.style.actions" => {
                self.analytics(args, credential, "style", "actions").await?
            }
            "cloud.library_analytics.style.usages" => {
                self.analytics(args, credential, "style", "usages").await?
            }
            "cloud.library_analytics.variable.actions" => {
                self.analytics(args, credential, "variable", "actions")
                    .await?
            }
            "cloud.library_analytics.variable.usages" => {
                self.analytics(args, credential, "variable", "usages")
                    .await?
            }
            "cloud.oembed.get" => self.oembed(args, credential).await?,
            _ => {
                return Err(Error::new(
                    ErrorCode::Unsupported,
                    "Unknown Figma REST capability",
                ));
            }
        };
        Ok(json!({
            "source":"figma_rest",
            "operation":operation,
            "operation_id":meta.operation_id,
            "scope":meta.scope,
            "deprecated":meta.deprecated,
            "data":data
        }))
    }

    fn url(&self, segments: &[&str], query: &[(&str, String)]) -> Result<Url> {
        let mut url = self.base.clone();
        {
            let mut path = url
                .path_segments_mut()
                .map_err(|_| Error::invalid("Figma REST URL cannot contain path segments"))?;
            path.clear();
            for segment in segments {
                if segment.is_empty()
                    || segment.len() > 512
                    || segment.chars().any(char::is_control)
                {
                    return Err(Error::invalid("Figma REST path segment exceeds bounds"));
                }
                path.push(segment);
            }
        }
        for (key, value) in query {
            if value.len() > 2048 || value.chars().any(char::is_control) {
                return Err(Error::invalid("Figma REST query value exceeds bounds"));
            }
            url.query_pairs_mut().append_pair(key, value);
        }
        Ok(url)
    }
    async fn request(
        &self,
        method: Method,
        url: Url,
        body: Option<Value>,
        credential: &Credential,
    ) -> Result<Value> {
        let mutating = method != Method::GET;
        let mut request = self.client.request(method, url);
        let mut header = match credential.kind {
            AuthKind::OAuth => {
                let mut bytes = Zeroizing::new(b"Bearer ".to_vec());
                bytes.extend_from_slice(&credential.token);
                HeaderValue::from_bytes(&bytes)
            }
            AuthKind::Personal | AuthKind::Plan => HeaderValue::from_bytes(&credential.token),
        }
        .map_err(|_| Error::new(ErrorCode::PermissionDenied, "Invalid Figma credential"))?;
        header.set_sensitive(true);
        request = match credential.kind {
            AuthKind::OAuth => request.header(AUTHORIZATION, header),
            AuthKind::Personal | AuthKind::Plan => request.header("X-Figma-Token", header),
        };
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.map_err(|error| {
            let mapped = map_reqwest_error(error);
            if mutating { mapped.uncertain() } else { mapped }
        })?;
        let status = response.status();
        let retry_after = response
            .headers()
            .get("retry-after")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let upgrade = response
            .headers()
            .get("x-figma-upgrade-link")
            .and_then(|value| value.to_str().ok())
            .filter(|value| value.starts_with("https://"))
            .map(str::to_owned);
        let mut stream = response.bytes_stream();
        let mut bytes = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.map_err(map_reqwest_error)?;
            if bytes.len().saturating_add(chunk.len()) > MAX_RESPONSE_BYTES {
                return Err(Error::new(
                    ErrorCode::ResourceExhausted,
                    "Figma REST response exceeded bounded body limit",
                ));
            }
            bytes.extend_from_slice(&chunk);
        }
        if !status.is_success() {
            return Err(map_http_status(status.as_u16(), retry_after, upgrade));
        }
        if bytes.is_empty() {
            return Ok(json!({"ok":true}));
        }
        serde_json::from_slice(&bytes).map_err(|_| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Figma REST returned invalid JSON",
            )
        })
    }

    async fn get(
        &self,
        segments: &[&str],
        query: &[(&str, String)],
        credential: &Credential,
    ) -> Result<Value> {
        self.request(Method::GET, self.url(segments, query)?, None, credential)
            .await
    }
    async fn file_get(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let key = required(a, "fileKey")?;
        let mut q = Vec::new();
        push_str(&mut q, a, "version", "version")?;
        push_list(&mut q, a, "ids", "ids", 200)?;
        push_u64(&mut q, a, "depth", "depth", 99)?;
        if optional_bool(a, "geometry")? == Some(true) {
            q.push(("geometry", "paths".to_owned()));
        }
        push_list(&mut q, a, "pluginData", "plugin_data", 64)?;
        push_bool(&mut q, a, "branchData", "branch_data")?;
        self.get(&["v1", "files", key], &q, c).await
    }
    async fn file_metadata(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.get(&["v1", "files", required(a, "fileKey")?, "meta"], &[], c)
            .await
    }
    async fn file_nodes(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_list(&mut q, a, "ids", "ids", 200)?;
        push_str(&mut q, a, "version", "version")?;
        push_u64(&mut q, a, "depth", "depth", 99)?;
        if optional_bool(a, "geometry")? == Some(true) {
            q.push(("geometry", "paths".to_owned()));
        }
        push_list(&mut q, a, "pluginData", "plugin_data", 64)?;
        self.get(&["v1", "files", required(a, "fileKey")?, "nodes"], &q, c)
            .await
    }
    async fn images_render(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_list(&mut q, a, "ids", "ids", 200)?;
        push_str(&mut q, a, "version", "version")?;
        push_f64(&mut q, a, "scale", "scale", 0.01, 4.0)?;
        push_str(&mut q, a, "format", "format")?;
        for (input, wire) in [
            ("svgOutlineText", "svg_outline_text"),
            ("svgIncludeId", "svg_include_id"),
            ("svgIncludeNodeId", "svg_include_node_id"),
            ("svgSimplifyStroke", "svg_simplify_stroke"),
            ("contentsOnly", "contents_only"),
            ("useAbsoluteBounds", "use_absolute_bounds"),
        ] {
            push_bool(&mut q, a, input, wire)?;
        }
        self.get(&["v1", "images", required(a, "fileKey")?], &q, c)
            .await
    }
    async fn versions(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_u64(&mut q, a, "pageSize", "page_size", 50)?;
        push_u64(&mut q, a, "before", "before", u64::MAX)?;
        push_u64(&mut q, a, "after", "after", u64::MAX)?;
        self.get(&["v1", "files", required(a, "fileKey")?, "versions"], &q, c)
            .await
    }
    async fn comments(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        optional_bool(a, "asMarkdown")?.map(|v| q.push(("as_md", v.to_string())));
        self.get(&["v1", "files", required(a, "fileKey")?, "comments"], &q, c)
            .await
    }
    async fn comment_create(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let key = required(a, "fileKey")?;
        let mut body = Map::new();
        body.insert(
            "message".into(),
            Value::String(required(a, "message")?.to_owned()),
        );
        if let Some(id) = optional_str(a, "commentId")? {
            body.insert("comment_id".into(), Value::String(id.to_owned()));
        }
        if let Some(meta) = a.get("clientMeta") {
            body.insert("client_meta".into(), meta.clone());
        }
        self.request(
            Method::POST,
            self.url(&["v1", "files", key, "comments"], &[])?,
            Some(Value::Object(body)),
            c,
        )
        .await
    }
    async fn comment_delete(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.request(
            Method::DELETE,
            self.url(
                &[
                    "v1",
                    "files",
                    required(a, "fileKey")?,
                    "comments",
                    required(a, "commentId")?,
                ],
                &[],
            )?,
            None,
            c,
        )
        .await
    }
    async fn reactions(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        optional_str(a, "cursor")?.map(|v| q.push(("cursor", v.to_owned())));
        self.get(
            &[
                "v1",
                "files",
                required(a, "fileKey")?,
                "comments",
                required(a, "commentId")?,
                "reactions",
            ],
            &q,
            c,
        )
        .await
    }
    async fn reaction_add(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let body = json!({"emoji":required(a,"emoji")?});
        self.request(
            Method::POST,
            self.url(
                &[
                    "v1",
                    "files",
                    required(a, "fileKey")?,
                    "comments",
                    required(a, "commentId")?,
                    "reactions",
                ],
                &[],
            )?,
            Some(body),
            c,
        )
        .await
    }
    async fn reaction_delete(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let q = [("emoji", required(a, "emoji")?.to_owned())];
        self.request(
            Method::DELETE,
            self.url(
                &[
                    "v1",
                    "files",
                    required(a, "fileKey")?,
                    "comments",
                    required(a, "commentId")?,
                    "reactions",
                ],
                &q,
            )?,
            None,
            c,
        )
        .await
    }
    async fn team_folders(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.get(&["v2", "teams", required(a, "teamId")?, "folders"], &[], c)
            .await
    }
    async fn folder_files(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_bool(&mut q, a, "branchData", "branch_data")?;
        self.get(&["v2", "folders", required(a, "folderId")?, "files"], &q, c)
            .await
    }
    async fn team_asset(
        &self,
        a: &Map<String, Value>,
        c: &Credential,
        kind: &str,
    ) -> Result<Value> {
        let mut q = Vec::new();
        push_u64(&mut q, a, "pageSize", "page_size", 1000)?;
        push_u64(&mut q, a, "after", "after", u64::MAX)?;
        push_u64(&mut q, a, "before", "before", u64::MAX)?;
        self.get(&["v1", "teams", required(a, "teamId")?, kind], &q, c)
            .await
    }
    async fn file_asset(
        &self,
        a: &Map<String, Value>,
        c: &Credential,
        kind: &str,
    ) -> Result<Value> {
        self.get(&["v1", "files", required(a, "fileKey")?, kind], &[], c)
            .await
    }
    async fn project_files(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_bool(&mut q, a, "branchData", "branch_data")?;
        self.get(
            &["v1", "projects", required(a, "projectId")?, "files"],
            &q,
            c,
        )
        .await
    }
    async fn webhooks_list(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        optional_str(a, "context")?.map(|v| q.push(("context", v.to_owned())));
        optional_str(a, "contextId")?.map(|v| q.push(("context_id", v.to_owned())));
        optional_str(a, "planApiId")?.map(|v| q.push(("plan_api_id", v.to_owned())));
        optional_str(a, "cursor")?.map(|v| q.push(("cursor", v.to_owned())));
        self.get(&["v2", "webhooks"], &q, c).await
    }
    async fn webhook_get(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.get(&["v2", "webhooks", required(a, "webhookId")?], &[], c)
            .await
    }
    async fn webhook_create(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let body = webhook_body(a, true)?;
        self.request(
            Method::POST,
            self.url(&["v2", "webhooks"], &[])?,
            Some(body),
            c,
        )
        .await
    }
    async fn webhook_update(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let body = webhook_body(a, false)?;
        self.request(
            Method::PUT,
            self.url(&["v2", "webhooks", required(a, "webhookId")?], &[])?,
            Some(body),
            c,
        )
        .await
    }
    async fn webhook_delete(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.request(
            Method::DELETE,
            self.url(&["v2", "webhooks", required(a, "webhookId")?], &[])?,
            None,
            c,
        )
        .await
    }
    async fn webhook_requests(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.get(
            &["v2", "webhooks", required(a, "webhookId")?, "requests"],
            &[],
            c,
        )
        .await
    }
    async fn activity_logs(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_list(&mut q, a, "events", "events", 64)?;
        push_u64(&mut q, a, "startTime", "start_time", u64::MAX)?;
        push_u64(&mut q, a, "endTime", "end_time", u64::MAX)?;
        push_u64(&mut q, a, "limit", "limit", 1000)?;
        push_str(&mut q, a, "order", "order")?;
        self.get(&["v1", "activity_logs"], &q, c).await
    }
    async fn developer_logs(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut body = Map::new();
        for (input, wire) in [
            ("tokenType", "token_type"),
            ("eventSource", "event_source"),
            ("dateRange", "date_range"),
            ("cursor", "cursor"),
        ] {
            if let Some(v) = optional_str(a, input)? {
                body.insert(wire.into(), Value::String(v.to_owned()));
            }
        }
        for (input, wire) in [
            ("tokens", "token"),
            ("tokenNames", "token_name"),
            ("userEmails", "user_email"),
            ("ipAddresses", "ip_address"),
        ] {
            if let Some(v) = optional_string_list(a, input, 64)? {
                body.insert(wire.into(), Value::String(v.join(",")));
            }
        }
        if let Some(v) = optional_u64(a, "limit", 100)? {
            body.insert("limit".into(), json!(v));
        }
        self.request(
            Method::POST,
            self.url(&["v1", "developer_logs"], &[])?,
            Some(Value::Object(body)),
            c,
        )
        .await
    }
    async fn ai_usage(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = vec![
            ("start_date", required(a, "startDate")?.to_owned()),
            ("end_date", required(a, "endDate")?.to_owned()),
        ];
        push_str(&mut q, a, "userEmail", "user_email")?;
        push_u64(&mut q, a, "limit", "limit", 1000)?;
        push_str(&mut q, a, "cursor", "cursor")?;
        self.get(&["v1", "ai_usage", "daily"], &q, c).await
    }
    async fn payments(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        for (input, wire) in [
            ("pluginPaymentToken", "plugin_payment_token"),
            ("userId", "user_id"),
            ("communityFileId", "community_file_id"),
            ("pluginId", "plugin_id"),
            ("widgetId", "widget_id"),
        ] {
            push_str(&mut q, a, input, wire)?;
        }
        self.get(&["v1", "payments"], &q, c).await
    }
    async fn variables_mutate(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut body = Map::new();
        for key in [
            "variableCollections",
            "variableModes",
            "variables",
            "variableModeValues",
        ] {
            if let Some(v) = a.get(key) {
                body.insert(key.into(), v.clone());
            }
        }
        if body.is_empty() {
            return Err(Error::invalid(
                "Variable mutation requires at least one change array",
            ));
        }
        self.request(
            Method::POST,
            self.url(&["v1", "files", required(a, "fileKey")?, "variables"], &[])?,
            Some(Value::Object(body)),
            c,
        )
        .await
    }
    async fn dev_resources(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = Vec::new();
        push_list(&mut q, a, "nodeIds", "node_ids", 200)?;
        self.get(
            &["v1", "files", required(a, "fileKey")?, "dev_resources"],
            &q,
            c,
        )
        .await
    }
    async fn dev_resources_create(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let resources = a
            .get("resources")
            .cloned()
            .ok_or_else(|| Error::invalid("resources is required"))?;
        self.request(
            Method::POST,
            self.url(&["v1", "dev_resources"], &[])?,
            Some(json!({"dev_resources":resources})),
            c,
        )
        .await
    }
    async fn dev_resources_update(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let resources = a
            .get("resources")
            .cloned()
            .ok_or_else(|| Error::invalid("resources is required"))?;
        self.request(
            Method::PUT,
            self.url(&["v1", "dev_resources"], &[])?,
            Some(json!({"dev_resources":resources})),
            c,
        )
        .await
    }
    async fn dev_resource_delete(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        self.request(
            Method::DELETE,
            self.url(
                &[
                    "v1",
                    "files",
                    required(a, "fileKey")?,
                    "dev_resources",
                    required(a, "devResourceId")?,
                ],
                &[],
            )?,
            None,
            c,
        )
        .await
    }
    async fn analytics(
        &self,
        a: &Map<String, Value>,
        c: &Credential,
        kind: &str,
        metric: &str,
    ) -> Result<Value> {
        let mut q = vec![("group_by", required(a, "groupBy")?.to_owned())];
        push_str(&mut q, a, "cursor", "cursor")?;
        push_str(&mut q, a, "startDate", "start_date")?;
        push_str(&mut q, a, "endDate", "end_date")?;
        self.get(
            &[
                "v1",
                "analytics",
                "libraries",
                required(a, "fileKey")?,
                kind,
                metric,
            ],
            &q,
            c,
        )
        .await
    }
    async fn oembed(&self, a: &Map<String, Value>, c: &Credential) -> Result<Value> {
        let mut q = vec![("url", required(a, "url")?.to_owned())];
        push_u64(&mut q, a, "maxWidth", "maxwidth", 10_000)?;
        push_u64(&mut q, a, "maxHeight", "maxheight", 10_000)?;
        self.get(&["v1", "oembed"], &q, c).await
    }
}
fn required<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str> {
    let value = args
        .get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| Error::invalid(format!("{key} must be a string")))?;
    if value.is_empty() || value.len() > 2048 || value.chars().any(char::is_control) {
        return Err(Error::invalid(format!("{key} exceeds bounds")));
    }
    Ok(value)
}
fn optional_str<'a>(args: &'a Map<String, Value>, key: &str) -> Result<Option<&'a str>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(value))
            if value.len() <= 2048 && !value.chars().any(char::is_control) =>
        {
            Ok(Some(value))
        }
        _ => Err(Error::invalid(format!("{key} must be a bounded string"))),
    }
}
fn optional_bool(args: &Map<String, Value>, key: &str) -> Result<Option<bool>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Bool(value)) => Ok(Some(*value)),
        _ => Err(Error::invalid(format!("{key} must be boolean"))),
    }
}
fn optional_u64(args: &Map<String, Value>, key: &str, max: u64) -> Result<Option<u64>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value)) if value.as_u64().is_some_and(|v| v <= max) => {
            Ok(value.as_u64())
        }
        _ => Err(Error::invalid(format!(
            "{key} must be a bounded unsigned integer"
        ))),
    }
}
fn optional_f64(args: &Map<String, Value>, key: &str, min: f64, max: f64) -> Result<Option<f64>> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::Number(value))
            if value
                .as_f64()
                .is_some_and(|v| v.is_finite() && v >= min && v <= max) =>
        {
            Ok(value.as_f64())
        }
        _ => Err(Error::invalid(format!(
            "{key} must be a bounded finite number"
        ))),
    }
}
fn optional_string_list(
    args: &Map<String, Value>,
    key: &str,
    max: usize,
) -> Result<Option<Vec<String>>> {
    let Some(value) = args.get(key) else {
        return Ok(None);
    };
    let values = value
        .as_array()
        .ok_or_else(|| Error::invalid(format!("{key} must be an array")))?;
    if values.len() > max {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            format!("{key} exceeds item limit"),
        ));
    }
    values
        .iter()
        .map(|item| {
            let text = item
                .as_str()
                .ok_or_else(|| Error::invalid(format!("{key} entries must be strings")))?;
            if text.is_empty() || text.len() > 2048 || text.chars().any(char::is_control) {
                return Err(Error::invalid(format!("{key} entry exceeds bounds")));
            }
            Ok(text.to_owned())
        })
        .collect::<Result<Vec<_>>>()
        .map(Some)
}
fn push_str(
    q: &mut Vec<(&'static str, String)>,
    a: &Map<String, Value>,
    input: &str,
    wire: &'static str,
) -> Result<()> {
    if let Some(v) = optional_str(a, input)? {
        q.push((wire, v.to_owned()));
    }
    Ok(())
}
fn push_bool(
    q: &mut Vec<(&'static str, String)>,
    a: &Map<String, Value>,
    input: &str,
    wire: &'static str,
) -> Result<()> {
    if let Some(v) = optional_bool(a, input)? {
        q.push((wire, v.to_string()));
    }
    Ok(())
}
fn push_u64(
    q: &mut Vec<(&'static str, String)>,
    a: &Map<String, Value>,
    input: &str,
    wire: &'static str,
    max: u64,
) -> Result<()> {
    if let Some(v) = optional_u64(a, input, max)? {
        q.push((wire, v.to_string()));
    }
    Ok(())
}
fn push_f64(
    q: &mut Vec<(&'static str, String)>,
    a: &Map<String, Value>,
    input: &str,
    wire: &'static str,
    min: f64,
    max: f64,
) -> Result<()> {
    if let Some(v) = optional_f64(a, input, min, max)? {
        q.push((wire, v.to_string()));
    }
    Ok(())
}
fn push_list(
    q: &mut Vec<(&'static str, String)>,
    a: &Map<String, Value>,
    input: &str,
    wire: &'static str,
    max: usize,
) -> Result<()> {
    if let Some(v) = optional_string_list(a, input, max)? {
        q.push((wire, v.join(",")));
    }
    Ok(())
}
fn webhook_body(args: &Map<String, Value>, create: bool) -> Result<Value> {
    let mut body = Map::new();
    for (input, wire) in [
        ("eventType", "event_type"),
        ("context", "context"),
        ("contextId", "context_id"),
        ("endpoint", "endpoint"),
        ("passcode", "passcode"),
        ("status", "status"),
        ("description", "description"),
    ] {
        if let Some(value) = optional_str(args, input)? {
            body.insert(wire.into(), Value::String(value.to_owned()));
        }
    }
    if create {
        for required_key in ["eventType", "context", "contextId", "endpoint", "passcode"] {
            if !args.contains_key(required_key) {
                return Err(Error::invalid(format!("{required_key} is required")));
            }
        }
    }
    if body.is_empty() {
        return Err(Error::invalid(
            "Webhook update must change at least one field",
        ));
    }
    Ok(Value::Object(body))
}

fn credential_allowed(operation_id: &str, kind: AuthKind) -> bool {
    match operation_id {
        "getAiUsageDaily" | "getDeveloperLogs" => matches!(kind, AuthKind::Plan),
        "getActivityLogs" => matches!(kind, AuthKind::OAuth),
        "getPayments" => matches!(kind, AuthKind::Personal),
        "getMe"
        | "getOEmbed"
        | "postComment"
        | "deleteComment"
        | "postCommentReaction"
        | "deleteCommentReaction"
        | "postVariables" => {
            matches!(kind, AuthKind::OAuth | AuthKind::Personal)
        }
        _ => true,
    }
}

fn map_reqwest_error(error: reqwest::Error) -> Error {
    if error.is_timeout() {
        Error::new(ErrorCode::Timeout, "Figma REST request timed out")
    } else {
        Error::unavailable("Figma REST request failed")
    }
}
fn map_http_status(status: u16, retry_after: Option<String>, upgrade: Option<String>) -> Error {
    let message = match status {
        401 | 403 => "Figma REST credential is invalid or lacks required scope".to_owned(),
        404 => "Figma REST resource was not found".to_owned(),
        429 => format!(
            "Figma REST rate limited; retry_after={}; upgrade={}",
            retry_after.as_deref().unwrap_or("unknown"),
            upgrade.as_deref().unwrap_or("none")
        ),
        _ => format!("Figma REST request failed with HTTP {status}"),
    };
    let code = match status {
        401 | 403 => ErrorCode::PermissionDenied,
        404 => ErrorCode::NotFound,
        429 => ErrorCode::ResourceExhausted,
        _ => ErrorCode::BackendFailed,
    };
    Error::new(code, message)
}
fn credential_socket_present() -> bool {
    #[cfg(unix)]
    {
        let dir = std::fs::symlink_metadata(CREDENTIAL_DIR);
        let sock = std::fs::symlink_metadata(CREDENTIAL_SOCKET);
        return dir.is_ok_and(|m| m.is_dir()) && sock.is_ok_and(|m| m.file_type().is_socket());
    }
    #[cfg(not(unix))]
    {
        false
    }
}

#[cfg(unix)]
async fn read_credential() -> Result<Credential> {
    let dir = Path::new(CREDENTIAL_DIR);
    let socket_path = Path::new(CREDENTIAL_SOCKET);
    let uid = unsafe { libc::getuid() };
    let dm = std::fs::symlink_metadata(dir)
        .map_err(|_| Error::unavailable("Figma REST credential mount is unavailable"))?;
    if !dm.is_dir() || dm.uid() != uid || dm.mode() & 0o777 != 0o700 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe Figma credential directory",
        ));
    }
    let sm = std::fs::symlink_metadata(socket_path)
        .map_err(|_| Error::unavailable("Figma REST credential socket is unavailable"))?;
    if !sm.file_type().is_socket() || sm.uid() != uid || sm.mode() & 0o777 != 0o600 {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Unsafe Figma credential socket",
        ));
    }
    let mut socket = tokio::net::UnixStream::connect(socket_path)
        .await
        .map_err(|_| Error::unavailable("Could not connect to Figma credential socket"))?;
    if socket
        .peer_cred()
        .map_err(|_| Error::unavailable("Could not authenticate Figma credential peer"))?
        .uid()
        != uid
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Figma credential peer UID mismatch",
        ));
    }
    let size = socket
        .read_u32()
        .await
        .map_err(|_| Error::unavailable("Could not read Figma credential frame"))?
        as usize;
    if size < 4 || size > MAX_SECRET_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Figma credential frame exceeds bounds",
        ));
    }
    let mut bytes = Zeroizing::new(vec![0u8; size]);
    socket
        .read_exact(&mut bytes)
        .await
        .map_err(|_| Error::unavailable("Could not read Figma credential"))?;
    let split = bytes
        .iter()
        .position(|byte| *byte == b'\n')
        .ok_or_else(|| {
            Error::new(
                ErrorCode::ProtocolMismatch,
                "Invalid Figma credential frame",
            )
        })?;
    let kind = match &bytes[..split] {
        b"oauth" => AuthKind::OAuth,
        b"pat" => AuthKind::Personal,
        b"plan" => AuthKind::Plan,
        _ => {
            return Err(Error::new(
                ErrorCode::ProtocolMismatch,
                "Unsupported Figma credential kind",
            ));
        }
    };
    let token = &bytes[split + 1..];
    if token.is_empty()
        || token.len() > 2048
        || token.iter().any(|b| b.is_ascii_control() || !b.is_ascii())
    {
        return Err(Error::new(
            ErrorCode::PermissionDenied,
            "Invalid Figma credential bytes",
        ));
    }
    Ok(Credential {
        kind,
        token: Zeroizing::new(token.to_vec()),
    })
}

#[cfg(not(unix))]
async fn read_credential() -> Result<Credential> {
    Err(Error::unavailable(
        "Portable Figma REST credential transport is not available on this platform",
    ))
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_builder_encodes_untrusted_identifiers() {
        let client = RestClient::with_base_for_tests("https://api.figma.com/").unwrap();
        let url = client
            .url(&["v1", "files", "a/b?c"], &[("cursor", "x y".into())])
            .unwrap();
        assert_eq!(
            url.as_str(),
            "https://api.figma.com/v1/files/a%2Fb%3Fc?cursor=x+y"
        );
    }

    #[test]
    fn webhook_creation_requires_complete_shape() {
        let mut args = Map::new();
        args.insert("eventType".into(), json!("FILE_UPDATE"));
        assert!(webhook_body(&args, true).is_err());
    }

    #[test]
    fn cloud_status_never_contains_a_token() {
        let status = RestClient::with_base_for_tests("https://api.figma.com/")
            .unwrap()
            .status();
        let text = status.to_string();
        assert!(!text.to_ascii_lowercase().contains("figd_"));
        assert!(!text.to_ascii_lowercase().contains("bearer "));
        assert_eq!(status["token_exposed_to_agent"], false);
    }

    async fn capture_one(response: &'static str) -> (String, tokio::task::JoinHandle<String>) {
        use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let task = tokio::spawn(async move {
            let (mut stream, _) = listener.accept().await.unwrap();
            let mut bytes = vec![0u8; 16 * 1024];
            let n = stream.read(&mut bytes).await.unwrap();
            let body = response.as_bytes();
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).await.unwrap();
            stream.write_all(body).await.unwrap();
            String::from_utf8_lossy(&bytes[..n]).into_owned()
        });
        (format!("http://{addr}/"), task)
    }

    #[tokio::test]
    async fn oauth_request_is_allowlisted_encoded_and_enveloped() {
        let (base, server) = capture_one(r#"{"nodes":{"1:2":{"document":{"id":"1:2"}}}}"#).await;
        let client = RestClient::with_base_for_tests(&base).unwrap();
        let args = serde_json::from_value::<Map<String, Value>>(json!({
            "fileKey":"A/B?C","ids":["1:2","3:4"],"geometry":true
        }))
        .unwrap();
        let output = client
            .execute_with_test_token("cloud.file.nodes", &args, "oauth", b"TEST_ONLY_SECRET")
            .await
            .unwrap();
        let request = server.await.unwrap();
        assert!(request.starts_with("GET /v1/files/A%2FB%3FC/nodes?"));
        assert!(request.contains("ids=1%3A2%2C3%3A4"));
        assert!(request.contains("geometry=paths"));
        assert!(
            request
                .to_ascii_lowercase()
                .contains("authorization: bearer test_only_secret")
        );
        assert_eq!(output["source"], "figma_rest");
        assert_eq!(output["operation_id"], "getFileNodes");
        assert_eq!(output["data"]["nodes"]["1:2"]["document"]["id"], "1:2");
    }

    #[tokio::test]
    async fn plan_token_is_rejected_for_user_endpoint_without_network() {
        let client = RestClient::with_base_for_tests("http://127.0.0.1:9/").unwrap();
        let error = client
            .execute_with_test_token("cloud.user.current", &Map::new(), "plan", b"TEST_ONLY_PLAN")
            .await
            .unwrap_err();
        assert_eq!(error.code, ErrorCode::PermissionDenied);
    }

    #[tokio::test]
    async fn failed_mutation_has_uncertain_outcome() {
        let client = RestClient::with_base_for_tests("http://127.0.0.1:9/").unwrap();
        let args = serde_json::from_value::<Map<String, Value>>(json!({
            "fileKey":"file","message":"hello"
        }))
        .unwrap();
        let error = client
            .execute_with_test_token("cloud.comment.create", &args, "oauth", b"TEST_ONLY_SECRET")
            .await
            .unwrap_err();
        assert!(!error.outcome_known);
    }
}
