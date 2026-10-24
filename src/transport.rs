// Copyright (C) 2026 Open Source Robotics Foundation
// SPDX-License-Identifier: Apache-2.0
//! Pooled blocking HTTP transport with bounded buffered and file-backed bodies.
use crate::{Error, ErrorKind, ResourceLimits};
use percent_encoding::{NON_ALPHANUMERIC, percent_decode_str, utf8_percent_encode};
use reqwest::{Client, multipart};
use serde_json::{Value, json};
use std::{
    collections::BTreeMap,
    future::Future,
    io::{Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    pin::Pin,
    sync::{
        Arc, Mutex, OnceLock,
        atomic::{AtomicU64, Ordering},
    },
    task::{Context, Poll},
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug)]
pub(crate) enum Method {
    Get,
    Post,
    Delete,
    Put,
    Patch,
    PostForm,
    PatchForm,
}
#[derive(Clone, Debug)]
pub(crate) enum FormValue {
    Text(String),
    File {
        path: PathBuf,
        filename: Option<String>,
    },
}
#[derive(Clone, Debug)]
pub(crate) struct FormField {
    pub name: String,
    pub value: FormValue,
}
#[derive(Clone, Debug)]
pub(crate) struct Request {
    pub method: Method,
    /// Complete endpoint, including an already encoded route and query.
    pub url: String,
    pub headers: Vec<String>,
    pub user_agent: String,
    pub body: Vec<u8>,
    pub form: Vec<FormField>,
}
#[derive(Debug)]
pub(crate) struct Response {
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub data: Vec<u8>,
    /// Compatibility adapter diagnostic. Typed requests return `Error` directly.
    pub error: Option<String>,
}
#[derive(Debug)]
pub(crate) struct DownloadResponse {
    pub final_url: String,
    pub status: u16,
    pub headers: BTreeMap<String, String>,
    pub body: tempfile::NamedTempFile,
}
impl DownloadResponse {
    pub(crate) fn header(&self, name: &str) -> Option<&str> {
        self.headers
            .get(&name.to_ascii_lowercase())
            .map(String::as_str)
    }
}

/// Shared allowance for every response body in a single graph operation.
#[derive(Clone, Debug)]
pub(crate) struct DownloadBudget(Arc<AtomicU64>);
impl DownloadBudget {
    pub(crate) fn new(bytes: u64) -> Self {
        Self(Arc::new(AtomicU64::new(bytes)))
    }
    fn consume(&self, bytes: u64) -> Result<(), Error> {
        self.0
            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |remaining| {
                remaining.checked_sub(bytes)
            })
            .map(|_| ())
            .map_err(|_| {
                error(
                    ErrorKind::Limit,
                    "operation exceeds configured total transfer byte limit",
                )
            })
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Ord, PartialOrd)]
struct ClientKey {
    connect: u64,
    request: Option<u64>,
    read: Option<u64>,
    caller_headers: bool,
}
static CLIENTS: OnceLock<Mutex<BTreeMap<ClientKey, Client>>> = OnceLock::new();
fn error(kind: ErrorKind, message: impl Into<String>) -> Error {
    Error::new(kind, message)
}
fn client(limits: &ResourceLimits, caller_headers: bool) -> Result<Client, Error> {
    let key = ClientKey {
        connect: limits.connect_timeout_ms,
        request: limits.request_timeout_ms,
        read: limits.read_timeout_ms,
        caller_headers,
    };
    let mut clients = CLIENTS
        .get_or_init(|| Mutex::new(BTreeMap::new()))
        .lock()
        .map_err(|_| error(ErrorKind::Transport, "HTTP client pool lock poisoned"))?;
    if let Some(client) = clients.get(&key) {
        return Ok(client.clone());
    }
    // Clients carry neither request headers nor credentials. Redirects are
    // processed by open_response so every hop uses the current request's guard
    // and streamed bodies can be reconstructed without a second HTTP stack.
    let builder = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(key.connect))
        .redirect(reqwest::redirect::Policy::none());
    let builder = if let Some(timeout) = key.request {
        builder.timeout(Duration::from_millis(timeout))
    } else {
        builder
    };
    let built = builder.build().map_err(|e| {
        Error::with_source(ErrorKind::Transport, "HTTP client initialization failed", e)
    })?;
    // Configuration profiles are caller-controlled; bound retained pools as well.
    if clients.len() >= 16 {
        clients.pop_first();
    }
    clients.insert(key, built.clone());
    Ok(built)
}
pub(crate) fn mime(path: impl AsRef<Path>) -> &'static str {
    let path = path.as_ref();
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
        .as_str()
    {
        "config" | "dae" | "sdf" | "urdf" | "xml" => "text/xml",
        "gif" => "image/gif",
        "htm" | "html" => "text/html",
        "jpg" | "jpeg" => "image/jpeg",
        "json" | "pbtxt" => "text/json",
        "pdf" => "application/pdf",
        "png" => "image/png",
        "svg" => "image/svg+xml",
        "tga" => "image/tga",
        "bvh" | "erb" | "obj" | "glsl" | "material" | "md" | "mtl" | "stl" | "txt" => "text/plain",
        _ => "application/octet-stream",
    }
}
static RUNTIME: OnceLock<Result<tokio::runtime::Runtime, String>> = OnceLock::new();
fn runtime() -> Result<&'static tokio::runtime::Runtime, Error> {
    RUNTIME
        .get_or_init(|| {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(1)
                .thread_name("fuel-http")
                .enable_all()
                .build()
                .map_err(|e| e.to_string())
        })
        .as_ref()
        .map_err(|message| {
            error(
                ErrorKind::Transport,
                format!("HTTP runtime initialization failed: {message}"),
            )
        })
}
fn run<T: Send>(task: impl Future<Output = Result<T, Error>> + Send) -> Result<T, Error> {
    let runtime = runtime()?;
    if tokio::runtime::Handle::try_current().is_ok() {
        // The public API is synchronous, including when invoked by async code.
        // Poll on a scoped helper instead of nesting a runtime on the caller.
        std::thread::scope(|scope| {
            scope
                .spawn(move || runtime.block_on(task))
                .join()
                .unwrap_or_else(|_| Err(error(ErrorKind::Transport, "HTTP worker panicked")))
        })
    } else {
        runtime.block_on(task)
    }
}
type Progress = Arc<Mutex<Instant>>;
/// Delay opening a multipart source until its body is polled. Multipart forms
/// retain every part until transmission, so eagerly opening all sources would
/// make the supported file count depend on the process descriptor limit.
struct LazyFile {
    path: PathBuf,
    remaining: u64,
    opening: Option<tokio::task::JoinHandle<std::io::Result<tokio::fs::File>>>,
    file: Option<tokio::fs::File>,
}
impl LazyFile {
    fn new(path: PathBuf, length: u64) -> Self {
        Self {
            path,
            remaining: length,
            opening: None,
            file: None,
        }
    }
}
impl Drop for LazyFile {
    fn drop(&mut self) {
        if let Some(opening) = &self.opening {
            opening.abort();
        }
    }
}
impl tokio::io::AsyncRead for LazyFile {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut tokio::io::ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        let this = self.get_mut();
        if this.remaining == 0 || buffer.remaining() == 0 {
            return Poll::Ready(Ok(()));
        }
        if this.file.is_none() {
            let opening = this.opening.get_or_insert_with(|| {
                let path = this.path.clone();
                tokio::spawn(async move { tokio::fs::File::open(path).await })
            });
            let file = match Pin::new(opening).poll(cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(result) => result.map_err(std::io::Error::other).and_then(|file| file),
            };
            this.opening = None;
            match file {
                Ok(file) => this.file = Some(file),
                Err(error) => return Poll::Ready(Err(error)),
            }
        }
        let before = buffer.filled().len();
        let result =
            Pin::new(this.file.as_mut().expect("source opened above")).poll_read(cx, buffer);
        if let Poll::Ready(ref result) = result {
            let read = buffer.filled().len() - before;
            this.remaining = this.remaining.saturating_sub(read as u64);
            // Release the source as soon as its last declared byte is emitted;
            // this does not depend on when reqwest drops its preceding parts.
            if this.remaining == 0 || read == 0 || result.is_err() {
                this.file = None;
            }
        }
        result
    }
}
struct UploadBody<R> {
    reader: R,
    remaining: u64,
    progress: Progress,
}
impl<R: tokio::io::AsyncRead + Unpin> http_body::Body for UploadBody<R> {
    type Data = bytes::Bytes;
    type Error = std::io::Error;
    fn poll_frame(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
    ) -> Poll<Option<Result<http_body::Frame<Self::Data>, Self::Error>>> {
        let this = self.get_mut();
        if this.remaining == 0 {
            return Poll::Ready(None);
        }
        let mut buffer = [0; 64 * 1024];
        let length = this.remaining.min(buffer.len() as u64) as usize;
        let mut target = tokio::io::ReadBuf::new(&mut buffer[..length]);
        match Pin::new(&mut this.reader).poll_read(cx, &mut target) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => Poll::Ready(Some(Err(error))),
            Poll::Ready(Ok(())) => {
                let bytes = target.filled();
                if bytes.is_empty() {
                    return Poll::Ready(Some(Err(std::io::Error::new(
                        std::io::ErrorKind::UnexpectedEof,
                        "upload file truncated during transfer",
                    ))));
                }
                this.remaining -= bytes.len() as u64;
                *this
                    .progress
                    .lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
                Poll::Ready(Some(Ok(http_body::Frame::data(
                    bytes::Bytes::copy_from_slice(bytes),
                ))))
            }
        }
    }
    fn size_hint(&self) -> http_body::SizeHint {
        http_body::SizeHint::with_exact(self.remaining)
    }
    fn is_end_stream(&self) -> bool {
        self.remaining == 0
    }
}
async fn response_once(
    input: &Request,
    limits: &ResourceLimits,
) -> Result<reqwest::Response, Error> {
    let method = match input.method {
        Method::Get => reqwest::Method::GET,
        Method::Post | Method::PostForm => reqwest::Method::POST,
        Method::Delete => reqwest::Method::DELETE,
        Method::Put => reqwest::Method::PUT,
        Method::Patch | Method::PatchForm => reqwest::Method::PATCH,
    };
    let client = client(limits, !input.headers.is_empty())?;
    let mut request = client
        .request(method, &input.url)
        .header(reqwest::header::USER_AGENT, &input.user_agent);
    for header in &input.headers {
        let (name, value) = header
            .split_once(':')
            .ok_or_else(|| error(ErrorKind::InvalidInput, "invalid HTTP header"))?;
        let name = reqwest::header::HeaderName::from_bytes(name.trim().as_bytes())
            .map_err(|e| Error::with_source(ErrorKind::InvalidInput, "invalid HTTP header", e))?;
        request = request.header(name, value.trim());
    }
    let progress = Arc::new(Mutex::new(Instant::now()));
    if matches!(input.method, Method::PostForm | Method::PatchForm) {
        let mut form = multipart::Form::new();
        for field in &input.form {
            let part = match &field.value {
                FormValue::Text(value) => {
                    form = form.text(field.name.clone(), value.clone());
                    continue;
                }
                FormValue::File { path, filename } => {
                    let metadata = tokio::fs::metadata(path).await.map_err(|e| {
                        Error::with_source(ErrorKind::Io, "cannot inspect multipart file", e)
                    })?;
                    if !metadata.is_file() {
                        return Err(error(
                            ErrorKind::InvalidInput,
                            "multipart source is not a regular file",
                        ));
                    }
                    let length = metadata.len();
                    let name = filename.clone().unwrap_or_else(|| {
                        path.file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned()
                    });
                    let body = reqwest::Body::wrap(UploadBody {
                        reader: LazyFile::new(path.clone(), length),
                        remaining: length,
                        progress: progress.clone(),
                    });
                    multipart::Part::stream_with_length(body, length)
                        .file_name(name)
                        .mime_str(mime(path))
                        .map_err(|e| {
                            Error::with_source(
                                ErrorKind::InvalidInput,
                                "invalid multipart content type",
                                e,
                            )
                        })?
                }
            };
            form = form.part(field.name.clone(), part);
        }
        request = request.multipart(form);
    } else if matches!(input.method, Method::Post | Method::Put | Method::Patch) {
        let length = input.body.len() as u64;
        request = request.body(reqwest::Body::wrap(UploadBody {
            reader: std::io::Cursor::new(input.body.clone()),
            remaining: length,
            progress: progress.clone(),
        }));
    }
    // Upload frames reset the pre-header inactivity allowance. A progressing
    // multipart upload can outlive this interval; the optional total deadline
    // is enforced independently by reqwest, including the response body.
    *progress
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Instant::now();
    let mut sending = Box::pin(request.send());
    let response = if let Some(idle) = limits.read_timeout_ms {
        let idle = Duration::from_millis(idle);
        loop {
            let elapsed = progress
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .elapsed();
            let remaining = idle.saturating_sub(elapsed);
            match tokio::time::timeout(remaining, sending.as_mut()).await {
                Ok(result) => break result,
                Err(_) => {
                    if progress
                        .lock()
                        .unwrap_or_else(std::sync::PoisonError::into_inner)
                        .elapsed()
                        >= idle
                    {
                        return Err(error(
                            ErrorKind::Transport,
                            "HTTP request made no progress before inactivity timeout",
                        ));
                    }
                }
            }
        }
    } else {
        sending.await
    };
    response.map_err(|e| Error::with_source(ErrorKind::Transport, "HTTP request failed", e))
}
async fn open_response(
    input: &Request,
    limits: &ResourceLimits,
) -> Result<reqwest::Response, Error> {
    let started = Instant::now();
    let mut redirected: Option<Request> = None;
    for hop in 0..=3 {
        let current = redirected.as_ref().unwrap_or(input);
        let mut remaining = limits.clone();
        if let Some(total) = limits.request_timeout_ms {
            let elapsed = started.elapsed().as_millis().min(u64::MAX as u128) as u64;
            remaining.request_timeout_ms = Some(
                total
                    .checked_sub(elapsed)
                    .filter(|n| *n > 0)
                    .ok_or_else(|| {
                        error(ErrorKind::Transport, "HTTP total request deadline exceeded")
                    })?,
            );
        }
        let response = response_once(current, &remaining).await?;
        // Rebuild owned body/file readers for method-preserving redirects.
        // Check credential scope before constructing the next request.
        if !matches!(response.status().as_u16(), 301 | 302 | 303 | 307 | 308) {
            return Ok(response);
        }
        let Some(location) = response.headers().get(reqwest::header::LOCATION) else {
            return Ok(response);
        };
        if hop == 3 {
            return Err(error(ErrorKind::Transport, "too many redirects"));
        }
        let previous = response.url();
        let next = previous
            .join(location.to_str().map_err(|e| {
                Error::with_source(ErrorKind::Transport, "invalid redirect Location", e)
            })?)
            .map_err(|e| Error::with_source(ErrorKind::Transport, "invalid redirect URL", e))?;
        if previous.scheme() == "https" && next.scheme() != "https" {
            return Err(error(
                ErrorKind::Transport,
                "HTTPS downgrade redirect refused",
            ));
        }
        if !current.headers.is_empty()
            && (
                previous.scheme(),
                previous.host_str(),
                previous.port_or_known_default(),
            ) != (next.scheme(), next.host_str(), next.port_or_known_default())
        {
            return Err(error(
                ErrorKind::Transport,
                "cross-origin redirect with caller headers refused",
            ));
        }
        let mut request = current.clone();
        if response.status().as_u16() == 303
            || (matches!(response.status().as_u16(), 301 | 302)
                && matches!(current.method, Method::Post | Method::PostForm))
        {
            request.method = Method::Get;
            request.body.clear();
            request.form.clear();
            request.headers.retain(|header| {
                !header.split_once(':').is_some_and(|(name, _)| {
                    matches!(
                        name.trim().to_ascii_lowercase().as_str(),
                        "content-length" | "content-type" | "transfer-encoding"
                    )
                })
            });
        }
        request.url = next.into();
        redirected = Some(request);
    }
    Err(error(ErrorKind::Transport, "too many redirects"))
}
fn response_headers(response: &reqwest::Response) -> Result<BTreeMap<String, String>, Error> {
    let mut headers = BTreeMap::<String, String>::new();
    for (key, raw) in response.headers() {
        let mut value = raw
            .to_str()
            .map_err(|e| {
                error(
                    ErrorKind::Transport,
                    format!("invalid HTTP response header: {e}"),
                )
            })?
            .trim()
            .to_owned();
        if key.as_str() == "link"
            && let Some(previous) = headers.get("link")
        {
            value = format!("{previous}, {value}");
        }
        headers.insert(key.to_string(), value.clone());
        let conventional = key
            .as_str()
            .split('-')
            .map(|p| {
                let mut c = p.chars();
                c.next()
                    .map(|h| h.to_ascii_uppercase().to_string() + c.as_str())
                    .unwrap_or_default()
            })
            .collect::<Vec<_>>()
            .join("-");
        headers.insert(conventional, value);
    }
    Ok(headers)
}
async fn copy_response(
    response: &mut reqwest::Response,
    writer: &mut (impl Write + Send),
    limit: u64,
    limits: &ResourceLimits,
    budget: Option<&DownloadBudget>,
) -> Result<(), Error> {
    let mut received = 0_u64;
    loop {
        let next = if let Some(timeout) = limits.read_timeout_ms {
            tokio::time::timeout(Duration::from_millis(timeout), response.chunk())
                .await
                .map_err(|e| {
                    Error::with_source(ErrorKind::Transport, "HTTP body inactivity timeout", e)
                })?
        } else {
            response.chunk().await
        };
        let Some(chunk) =
            next.map_err(|e| Error::with_source(ErrorKind::Transport, "HTTP body read failed", e))?
        else {
            return Ok(());
        };
        received = received
            .checked_add(chunk.len() as u64)
            .filter(|n| *n <= limit)
            .ok_or_else(|| error(ErrorKind::Limit, "HTTP body exceeds configured byte limit"))?;
        if let Some(budget) = budget {
            budget.consume(chunk.len() as u64)?;
        }
        writer
            .write_all(&chunk)
            .map_err(|e| Error::with_source(ErrorKind::Io, "cannot store HTTP body", e))?;
    }
}
fn check_length(response: &reqwest::Response, limit: u64) -> Result<(), Error> {
    if response
        .content_length()
        .is_some_and(|length| length > limit)
    {
        return Err(error(
            ErrorKind::Limit,
            "HTTP Content-Length exceeds configured byte limit",
        ));
    }
    Ok(())
}
pub(crate) fn send(input: &Request, limits: &ResourceLimits) -> Result<Response, Error> {
    send_buffered(input, limits, limits.max_metadata_bytes)
}
fn send_buffered(
    input: &Request,
    limits: &ResourceLimits,
    maximum: u64,
) -> Result<Response, Error> {
    run(async {
        let mut response = open_response(input, limits).await?;
        check_length(&response, maximum)?;
        let status = response.status().as_u16();
        let headers = response_headers(&response)?;
        let mut data = Vec::new();
        copy_response(&mut response, &mut data, maximum, limits, None).await?;
        Ok(Response {
            status,
            headers,
            data,
            error: None,
        })
    })
}
pub(crate) fn send_download_with_budget(
    input: &Request,
    limits: &ResourceLimits,
    budget: &DownloadBudget,
) -> Result<DownloadResponse, Error> {
    run(async {
        let mut response = open_response(input, limits).await?;
        let metadata = !response.status().is_success()
            || response
                .headers()
                .get(reqwest::header::CONTENT_TYPE)
                .and_then(|value| value.to_str().ok())
                .is_some_and(|value| {
                    value
                        .split(';')
                        .next()
                        .unwrap_or("")
                        .trim()
                        .eq_ignore_ascii_case("text/plain")
                });
        let maximum = if metadata {
            limits.max_metadata_bytes.min(limits.max_download_bytes)
        } else {
            limits.max_download_bytes
        };
        check_length(&response, maximum)?;
        let status = response.status().as_u16();
        let final_url = response.url().to_string();
        let headers = response_headers(&response)?;
        let mut body = tempfile::NamedTempFile::new()
            .map_err(|e| Error::with_source(ErrorKind::Io, "cannot stage HTTP download", e))?;
        copy_response(&mut response, &mut body, maximum, limits, Some(budget)).await?;
        body.seek(SeekFrom::Start(0))
            .map_err(|e| Error::with_source(ErrorKind::Io, "cannot rewind HTTP download", e))?;
        Ok(DownloadResponse {
            final_url,
            status,
            headers,
            body,
        })
    })
}
fn s<'a>(v: &'a Value, key: &str) -> &'a str {
    v[key].as_str().unwrap_or("")
}
pub(crate) fn join_url(a: &str, b: &str) -> String {
    if a.is_empty() {
        b.into()
    } else if b.is_empty() {
        a.into()
    } else {
        format!("{}/{}", a.trim_end_matches('/'), b.trim_start_matches('/'))
    }
}
fn legacy_request(a: &Value, body: &[u8], legacy_files: bool) -> Result<Request, Error> {
    let mut endpoint = join_url(s(a, "url"), s(a, "version"));
    if !s(a, "path").is_empty() {
        const SEGMENT: &percent_encoding::AsciiSet = &NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'_')
            .remove(b'.')
            .remove(b'~');
        let path = if a["path_encoded"] == true {
            s(a, "path").to_owned()
        } else {
            s(a, "path")
                .split('/')
                .map(|part| {
                    percent_decode_str(part)
                        .decode_utf8()
                        .map(|decoded| utf8_percent_encode(&decoded, SEGMENT).to_string())
                })
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| error(ErrorKind::InvalidInput, format!("invalid URL path: {e}")))?
                .join("/")
        };
        endpoint = join_url(&endpoint, &path);
    }
    let query = a["query"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .collect::<Vec<_>>()
        .join("&");
    if !query.is_empty() {
        endpoint.push('?');
        endpoint.push_str(&query);
    }
    let method = match a["method"].as_u64().unwrap_or(0) {
        0 => Method::Get,
        1 => Method::Post,
        2 => Method::Delete,
        3 => Method::Put,
        4 => Method::Patch,
        5 => Method::PostForm,
        6 => Method::PatchForm,
        _ => return Err(error(ErrorKind::InvalidInput, "unknown HTTP method")),
    };
    let mut form = Vec::new();
    for pair in a["form"].as_array().into_iter().flatten() {
        let name = pair[0]
            .as_str()
            .ok_or_else(|| error(ErrorKind::InvalidInput, "invalid form field"))?
            .to_owned();
        let value = if let Some(path) = pair[1]["file"].as_str() {
            FormValue::File {
                path: path.into(),
                filename: pair[1]["filename"].as_str().map(str::to_owned),
            }
        } else {
            let value = pair[1]
                .as_str()
                .ok_or_else(|| error(ErrorKind::InvalidInput, "invalid form value"))?;
            if let Some(file) = value.strip_prefix('@').filter(|_| legacy_files) {
                let (path, filename) = file.split_once(';').unwrap_or((file, ""));
                FormValue::File {
                    path: path.into(),
                    filename: (!filename.is_empty()).then(|| filename.to_owned()),
                }
            } else {
                FormValue::Text(value.into())
            }
        };
        form.push(FormField { name, value });
    }
    Ok(Request {
        method,
        url: endpoint,
        headers: a["headers"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect(),
        user_agent: s(a, "user_agent").into(),
        body: body.to_vec(),
        form,
    })
}
fn legacy_limits(a: &Value) -> ResourceLimits {
    serde_json::from_value(a["config"]["limits"].clone())
        .or_else(|_| serde_json::from_value(a["limits"].clone()))
        .unwrap_or_default()
}
fn failed(e: Error) -> Response {
    Response {
        status: 0,
        headers: Default::default(),
        data: vec![],
        error: Some(e.to_string()),
    }
}
pub(crate) fn transport(a: &Value) -> Response {
    let data: Vec<_> = a["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_else(|| s(a, "data").as_bytes().to_vec());
    legacy_request(a, &data, false)
        .and_then(|request| send(&request, &legacy_limits(a)))
        .unwrap_or_else(failed)
}
/// C++ binary body adapter. JSON contains control metadata only.
pub(crate) fn request_binary(a: &Value, body: &[u8]) -> (Value, Vec<u8>) {
    let response = legacy_request(a, body, true)
        .and_then(|request| {
            let limits = legacy_limits(a);
            send_buffered(&request, &limits, limits.max_download_bytes)
        })
        .unwrap_or_else(failed);
    (
        json!({"status":response.status,"headers":response.headers,"error":response.error}),
        response.data,
    )
}
pub(crate) fn request(a: &Value) -> Value {
    let data: Vec<_> = a["data"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_u64().and_then(|n| u8::try_from(n).ok()))
                .collect()
        })
        .unwrap_or_else(|| s(a, "data").as_bytes().to_vec());
    let (mut metadata, body) = request_binary(a, &data);
    metadata["data"] = json!(body);
    metadata
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::{net::TcpListener, thread};

    fn fixture(body: &[u8], headers: &str, action: impl FnOnce(String)) -> Vec<u8> {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let body = body.to_vec();
        let headers = headers.to_owned();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut received = Vec::new();
            loop {
                let mut buf = [0; 8192];
                let count = socket.read(&mut buf).unwrap();
                if count == 0 {
                    break;
                }
                received.extend_from_slice(&buf[..count]);
                if let Some(end) = received.windows(4).position(|v| v == b"\r\n\r\n") {
                    let head = String::from_utf8_lossy(&received[..end]).to_ascii_lowercase();
                    let size = head
                        .lines()
                        .find_map(|l| {
                            l.strip_prefix("content-length:")
                                .and_then(|n| n.trim().parse::<usize>().ok())
                        })
                        .unwrap_or(0);
                    if received.len() >= end + 4 + size {
                        break;
                    }
                }
            }
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\n{}Connection: close\r\n\r\n",
                body.len(),
                headers
            );
            let _ = socket.write_all(header.as_bytes());
            let _ = socket.write_all(&body);
            received
        });
        action(url);
        server.join().unwrap()
    }
    fn send_download(input: &Request, limits: &ResourceLimits) -> Result<DownloadResponse, Error> {
        send_download_with_budget(
            input,
            limits,
            &DownloadBudget::new(limits.max_total_download_bytes),
        )
    }
    fn request(url: String) -> Request {
        Request {
            method: Method::Get,
            url,
            headers: vec![],
            user_agent: "typed-test".into(),
            body: vec![],
            form: vec![],
        }
    }

    #[test]
    fn typed_bodies_are_bounded_and_downloads_are_file_backed() {
        let binary = vec![0, 255, 128, 13, 10, 0];
        fixture(
            &binary,
            "Link: </a?page=2>; rel=next\r\nLink: </a?page=1>; rel=prev\r\n",
            |url| {
                let response = send(&request(url), &ResourceLimits::default()).unwrap();
                assert_eq!(response.data, binary);
                assert!(response.headers.get("Link").unwrap().contains("next"));
                assert!(response.headers.get("link").unwrap().contains("prev"));
                assert!(!response.headers.contains_key("unknown"));
            },
        );
        fixture(&binary, "", |url| {
            let mut response = send_download(&request(url), &ResourceLimits::default()).unwrap();
            assert_eq!(response.status, 200);
            assert_eq!(response.header("Content-Length"), Some("6"));
            let mut data = Vec::new();
            response.body.read_to_end(&mut data).unwrap();
            assert_eq!(data, binary);
        });
        fixture(&binary, "", |url| {
            let limits = ResourceLimits {
                max_metadata_bytes: 3,
                ..Default::default()
            };
            assert_eq!(
                send(&request(url), &limits).unwrap_err().kind,
                ErrorKind::Limit
            );
        });
    }
    fn chunked(body: &[u8], headers: &str, action: impl FnOnce(String)) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let response=[format!("HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n{headers}Connection: close\r\n\r\n{:x}\r\n",body.len()).into_bytes(),body.to_vec(),b"\r\n0\r\n\r\n".to_vec()].concat();
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            let mut input = [0; 4096];
            let _ = socket.read(&mut input);
            let _ = socket.write_all(&response);
        });
        action(url);
        server.join().unwrap();
    }
    #[test]
    fn unknown_lengths_respect_metadata_download_and_shared_limits() {
        let limits = ResourceLimits {
            max_metadata_bytes: 3,
            max_download_bytes: 100,
            ..Default::default()
        };
        chunked(b"123456", "Content-Type: text/plain\r\n", |url| {
            assert_eq!(
                send_download(&request(url), &limits).unwrap_err().kind,
                ErrorKind::Limit
            );
        });
        chunked(b"123456", "Content-Type: application/zip\r\n", |url| {
            assert_eq!(
                send_download(
                    &request(url),
                    &ResourceLimits {
                        max_download_bytes: 3,
                        ..limits.clone()
                    }
                )
                .unwrap_err()
                .kind,
                ErrorKind::Limit
            );
        });
        let budget = DownloadBudget::new(5);
        for expected in [true, false] {
            chunked(b"123", "Content-Type: application/zip\r\n", |url| {
                let result = send_download_with_budget(&request(url), &limits, &budget);
                assert_eq!(result.is_ok(), expected);
                if let Err(error) = result {
                    assert_eq!(error.kind, ErrorKind::Limit);
                }
            });
        }
        chunked(b"123456", "", |url| {
            assert_eq!(
                send(&request(url), &limits).unwrap_err().kind,
                ErrorKind::Limit
            );
        });
    }
    #[test]
    fn synchronous_api_is_safe_inside_an_async_runtime() {
        fixture(b"ok", "", |url| {
            let result = runtime()
                .unwrap()
                .block_on(async { send(&request(url), &ResourceLimits::default()) });
            assert_eq!(result.unwrap().data, b"ok");
        });
    }
    #[cfg(unix)]
    #[test]
    fn multipart_many_files_with_low_descriptor_limit() {
        const CHILD: &str = "FUEL_MULTIPART_LOW_FD_CHILD";
        if std::env::var_os(CHILD).is_none() {
            // Change the descriptor limit only in this dedicated child. Other
            // tests and concurrent HTTP clients retain their normal limits.
            let output = std::process::Command::new("sh")
                .args([
                    "-c",
                    "ulimit -n 128 || exit 99\nexec \"$@\"",
                    "fuel-upload-limit",
                ])
                .arg(std::env::current_exe().unwrap())
                .args([
                    "--exact",
                    "transport::tests::multipart_many_files_with_low_descriptor_limit",
                    "--nocapture",
                ])
                .env(CHILD, "1")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "low-descriptor child failed: {}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            assert!(
                String::from_utf8_lossy(&output.stdout).contains("running 1 test"),
                "child test filter did not run the upload regression"
            );
            return;
        }
        use std::os::unix::ffi::OsStringExt;
        let root = tempfile::tempdir().unwrap();
        let model = root
            .path()
            .join(std::ffi::OsString::from_vec(b"model-root-\xff".to_vec()));
        std::fs::create_dir(&model).unwrap();
        std::fs::write(
            model.join("model.config"),
            "<model><name>many</name><sdf version='1.10'>model.sdf</sdf></model>",
        )
        .unwrap();
        std::fs::write(
            model.join("model.sdf"),
            "<sdf version='1.10'><model name='many'/></sdf>",
        )
        .unwrap();
        for index in 0..300 {
            std::fs::write(
                model.join(format!("part-{index:03}.bin")),
                [index as u8, 0, 255, 128],
            )
            .unwrap();
        }
        let received = fixture(b"ok", "", |url| {
            let client = crate::FuelClient::new(crate::ClientConfig::default());
            let id = crate::ResourceId {
                name: "many".into(),
                owner: "alice".into(),
                server: crate::ServerConfig {
                    url,
                    ..Default::default()
                },
                ..Default::default()
            };
            client.upload(&model, &id, &[], false, "").unwrap();
        });
        assert_eq!(
            received
                .windows(b"filename=\"".len())
                .filter(|bytes| *bytes == b"filename=\"")
                .count(),
            302
        );
        for index in 0..300 {
            let marker = format!("filename=\"part-{index:03}.bin\"");
            let start = received
                .windows(marker.len())
                .position(|bytes| bytes == marker.as_bytes())
                .unwrap();
            assert_eq!(
                received
                    .windows(marker.len())
                    .filter(|bytes| *bytes == marker.as_bytes())
                    .count(),
                1
            );
            let payload = start
                + received[start..]
                    .windows(4)
                    .position(|bytes| bytes == b"\r\n\r\n")
                    .unwrap()
                + 4;
            assert_eq!(&received[payload..payload + 4], &[index as u8, 0, 255, 128]);
        }
    }
    #[test]
    fn multipart_progress_outlives_inactivity_but_obeys_whole_deadline() {
        fn upload(total: Option<u64>) -> Result<Response, Error> {
            let file = tempfile::NamedTempFile::new().unwrap();
            file.as_file().set_len(16 * 1024 * 1024).unwrap();
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut received = 0;
                let mut expected = None;
                let mut header = Vec::new();
                loop {
                    let mut bytes = [0; 64 * 1024];
                    let count = match socket.read(&mut bytes) {
                        Ok(count) => count,
                        Err(_) => break,
                    };
                    if count == 0 {
                        break;
                    }
                    received += count;
                    if expected.is_none() {
                        header.extend_from_slice(&bytes[..count]);
                        if let Some(end) = header.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                        {
                            let text = String::from_utf8_lossy(&header[..end]).to_ascii_lowercase();
                            let length = text
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap();
                            expected = Some(end + 4 + length);
                        }
                    }
                    if expected.is_some_and(|length| received >= length) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(2));
                }
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                );
            });
            let mut input = request(url);
            input.method = Method::PostForm;
            input.form.push(FormField {
                name: "file".into(),
                value: FormValue::File {
                    path: file.path().into(),
                    filename: None,
                },
            });
            let result = send(
                &input,
                &ResourceLimits {
                    read_timeout_ms: Some(150),
                    request_timeout_ms: total,
                    ..Default::default()
                },
            );
            server.join().unwrap();
            result
        }
        assert_eq!(upload(None).unwrap().data, b"ok");
        assert_eq!(upload(Some(50)).unwrap_err().kind, ErrorKind::Transport);
    }
    #[test]
    fn multipart_files_are_readers_and_binary_text_is_literal() {
        let file = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(file.path(), [0, 255, 128, b'X']).unwrap();
        let received = fixture(b"ok", "", |url| {
            let mut input = request(url);
            input.method = Method::PostForm;
            input.form = vec![
                FormField {
                    name: "file".into(),
                    value: FormValue::File {
                        path: file.path().into(),
                        filename: Some("renamed.dat".into()),
                    },
                },
                FormField {
                    name: "literal".into(),
                    value: FormValue::Text("@a-path-is-text".into()),
                },
            ];
            assert_eq!(
                send(&input, &ResourceLimits::default()).unwrap().data,
                b"ok"
            );
        });
        assert!(received.windows(4).any(|v| v == [0, 255, 128, b'X']));
        let text = String::from_utf8_lossy(&received);
        assert!(text.contains("renamed.dat"));
        assert!(text.contains("@a-path-is-text"));
        let mut missing = request("http://127.0.0.1:1".into());
        missing.method = Method::PatchForm;
        missing.form.push(FormField {
            name: "file".into(),
            value: FormValue::File {
                path: "/missing/fuel-file".into(),
                filename: None,
            },
        });
        assert_eq!(
            send(&missing, &ResourceLimits::default()).unwrap_err().kind,
            ErrorKind::Io
        );
    }
    #[test]
    fn progressing_downloads_have_inactivity_and_optional_total_timeouts() {
        fn progressing(limits: ResourceLimits) -> Result<DownloadResponse, Error> {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let (mut socket, _) = listener.accept().unwrap();
                socket
                    .set_read_timeout(Some(Duration::from_secs(5)))
                    .unwrap();
                let mut input = [0; 4096];
                let _ = socket.read(&mut input);
                let _ = socket.write_all(
                    b"HTTP/1.1 200 OK\r\nContent-Length: 8\r\nConnection: close\r\n\r\n",
                );
                for _ in 0..8 {
                    if socket.write_all(b"x").is_err() {
                        break;
                    }
                    thread::sleep(Duration::from_millis(20));
                }
            });
            let result = send_download(&request(url), &limits);
            server.join().unwrap();
            result
        }
        let limits = ResourceLimits {
            read_timeout_ms: Some(100),
            request_timeout_ms: None,
            ..Default::default()
        };
        assert!(progressing(limits.clone()).is_ok());
        let failure = progressing(ResourceLimits {
            request_timeout_ms: Some(50),
            ..limits
        })
        .unwrap_err();
        assert_eq!(failure.kind, ErrorKind::Transport);
        assert!(std::error::Error::source(&failure).is_some());
    }
    #[test]
    fn sequential_requests_reuse_one_http_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(5)))
                .unwrap();
            for _ in 0..2 {
                let mut input = Vec::new();
                while !input.windows(4).any(|bytes| bytes == b"\r\n\r\n") {
                    let mut bytes = [0; 4096];
                    let count = socket.read(&mut bytes).unwrap();
                    assert_ne!(count, 0);
                    input.extend_from_slice(&bytes[..count]);
                }
                socket
                    .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\n\r\nok")
                    .unwrap();
            }
        });
        let input = request(url);
        let limits = ResourceLimits::default();
        assert_eq!(send(&input, &limits).unwrap().data, b"ok");
        assert_eq!(send(&input, &limits).unwrap().data, b"ok");
        server.join().unwrap();
    }
    #[test]
    fn redirects_replay_streamed_bodies_and_apply_method_changes() {
        for status in [302, 307] {
            let listener = TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let server = thread::spawn(move || {
                let mut requests = vec![];
                for index in 0..2 {
                    let (mut socket, _) = listener.accept().unwrap();
                    socket
                        .set_read_timeout(Some(Duration::from_secs(5)))
                        .unwrap();
                    let mut received = Vec::new();
                    loop {
                        let mut bytes = [0; 4096];
                        let count = socket.read(&mut bytes).unwrap();
                        if count == 0 {
                            break;
                        }
                        received.extend_from_slice(&bytes[..count]);
                        if let Some(end) =
                            received.windows(4).position(|bytes| bytes == b"\r\n\r\n")
                        {
                            let text =
                                String::from_utf8_lossy(&received[..end]).to_ascii_lowercase();
                            let size = text
                                .lines()
                                .find_map(|line| {
                                    line.strip_prefix("content-length:")
                                        .and_then(|value| value.trim().parse::<usize>().ok())
                                })
                                .unwrap_or(0);
                            if received.len() >= end + 4 + size {
                                break;
                            }
                        }
                    }
                    requests.push(received);
                    if index == 0 {
                        socket.write_all(format!("HTTP/1.1 {status} Redirect\r\nLocation: /target\r\nContent-Length: 0\r\nConnection: close\r\n\r\n").as_bytes()).unwrap();
                    } else {
                        socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok").unwrap();
                    }
                }
                requests
            });
            let mut input = request(url);
            input.method = Method::Post;
            input.body = vec![0, 255, 128];
            assert_eq!(
                send(&input, &ResourceLimits::default()).unwrap().data,
                b"ok"
            );
            let received = server.join().unwrap();
            assert!(received[0].ends_with(&[0, 255, 128]));
            if status == 307 {
                assert!(received[1].starts_with(b"POST /target "));
                assert!(received[1].ends_with(&[0, 255, 128]));
            } else {
                assert!(received[1].starts_with(b"GET /target "));
                assert!(!received[1].ends_with(&[0, 255, 128]));
            }
        }
    }
    #[test]
    fn response_inactivity_is_distinct_from_total_deadline() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let server = thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut input = [0; 4096];
            let _ = socket.read(&mut input);
            socket
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nx")
                .unwrap();
            thread::sleep(Duration::from_millis(100));
            let _ = socket.write_all(b"x");
        });
        let error = send(
            &request(url),
            &ResourceLimits {
                read_timeout_ms: Some(25),
                request_timeout_ms: None,
                ..Default::default()
            },
        )
        .unwrap_err();
        assert_eq!(error.kind, ErrorKind::Transport);
        assert!(std::error::Error::source(&error).is_some());
        server.join().unwrap();
    }
    #[test]
    fn client_profiles_are_reused_and_legacy_binary_is_not_json() {
        let limits = ResourceLimits::default();
        let _ = client(&limits, false).unwrap();
        let _ = client(&limits, false).unwrap();
        let _ = client(&limits, true).unwrap();
        for n in 1..=18 {
            let _ = client(
                &ResourceLimits {
                    connect_timeout_ms: n,
                    ..limits.clone()
                },
                false,
            )
            .unwrap();
        }
        assert!(CLIENTS.get().unwrap().lock().unwrap().len() <= 16);
        let response = fixture(&[0, 255, 0], "", |url| {
            let (metadata, body) = request_binary(&json!({"url":url,"method":1}), &[255, 0, 1]);
            assert_eq!(metadata["status"], 200);
            assert!(metadata.get("data").is_none());
            assert_eq!(body, [0, 255, 0]);
        });
        assert!(response.ends_with(&[255, 0, 1]));
        assert!(legacy_request(&json!({"method":99}), &[], true).is_err());
        assert!(legacy_request(&json!({"form":[[1,2]]}), &[], true).is_err());
        assert!(legacy_request(&json!({"form":[["a",2]]}), &[], true).is_err());
        assert!(legacy_request(&json!({"url":"http://a","path":"%ff"}), &[], true).is_err());
        let parsed =
            legacy_request(&json!({"form":[["a","@/tmp/foo;renamed"]]}), &[], true).unwrap();
        assert!(
            matches!(&parsed.form[0].value,FormValue::File{filename:Some(name),..} if name=="renamed")
        );
    }
}
