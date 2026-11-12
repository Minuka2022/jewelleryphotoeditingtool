//! Web version of Jewelry Studio. Starts a small web server that only this
//! computer can reach (127.0.0.1), serves the same interface as the desktop
//! app, and opens it in the browser.

use std::convert::Infallible;
use std::io::{Cursor, Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener as StdListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use axum::body::{Body, Bytes};
use axum::extract::{DefaultBodyLimit, Path as UrlPath, Query, Request, State};
use axum::http::{header, HeaderValue, Method, StatusCode, Uri};
use axum::middleware::{self, Next};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use axum::{Json, Router};
use futures_util::stream::Stream;
use rust_embed::RustEmbed;
use serde::Deserialize;
use serde_json::Value;
use studio_app::generate::Progress;
use studio_app::service::{self, Shared};
use studio_app::store::new_id;
use studio_app::studio::{OutputRef, Studio};
use studio_core::export::ExportOptions;
use tokio::sync::broadcast;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

/// The built interface (`app/dist`), compiled into the program so it is a
/// single file. Build the interface first: `npm run build` in `app`.
#[derive(RustEmbed)]
#[folder = "../../app/dist"]
struct Interface;

const PREFERRED_PORT: u16 = 8787;
const MAX_UPLOAD_BYTES: usize = 200 * 1024 * 1024;
/// Every request that changes something must carry this header. Browsers do
/// not let other websites add it, so they cannot use this server.
const REQUEST_HEADER: &str = "x-jewelry-studio";
/// Marks the page so the interface knows it talks to this server.
const PAGE_MARKER: &str = r#"<meta name="jewelry-studio" content="web">"#;

struct App {
    studio: Shared,
    data_dir: PathBuf,
    uploads: PathBuf,
    progress: broadcast::Sender<Progress>,
    allowed_hosts: Vec<String>,
}

type Ctx = Arc<App>;

#[derive(Debug)]
struct Failure(StatusCode, String);

impl IntoResponse for Failure {
    fn into_response(self) -> Response {
        (self.0, Json(Value::String(self.1))).into_response()
    }
}

fn bad(message: impl Into<String>) -> Failure {
    Failure(StatusCode::BAD_REQUEST, message.into())
}

// --- Operations ----------------------------------------------------------

async fn invoke(
    State(app): State<Ctx>,
    UrlPath(command): UrlPath<String>,
    body: Bytes,
) -> Result<Json<Value>, Failure> {
    let args: Value = if body.is_empty() {
        Value::Object(Default::default())
    } else {
        serde_json::from_slice(&body)
            .map_err(|e| bad(format!("The request is not valid JSON: {e}")))?
    };
    let value = match command.as_str() {
        "get_version" => Value::String(env!("CARGO_PKG_VERSION").to_owned()),
        // The web version is updated by starting the newest copy of the program.
        "check_for_update" => Value::Null,
        "install_update" => {
            return Err(bad(
                "The web version updates when you start the newest copy of the program.",
            ))
        }
        _ => {
            let progress = app.progress.clone();
            let outcome = service::dispatch(&app.studio, &command, args, move |event| {
                // Nobody listening is fine.
                let _ = progress.send(event);
            })
            .await;
            match outcome {
                Some(result) => result.map_err(bad)?,
                None => {
                    return Err(Failure(
                        StatusCode::NOT_FOUND,
                        format!("Unknown operation \"{command}\"."),
                    ))
                }
            }
        }
    };
    Ok(Json(value))
}

#[derive(Deserialize)]
struct UploadQuery {
    name: String,
}

/// Saves a file the browser sends and returns where it was saved, so the
/// other operations can use it like any file on disk. Each upload gets its
/// own folder, which keeps the original file name.
async fn upload(
    State(app): State<Ctx>,
    Query(query): Query<UploadQuery>,
    body: Bytes,
) -> Result<Json<String>, Failure> {
    if body.is_empty() {
        return Err(bad("The file is empty."));
    }
    let folder = app.uploads.join(new_id("u"));
    let path = folder.join(safe_file_name(&query.name));
    std::fs::create_dir_all(&folder)
        .and_then(|()| std::fs::write(&path, &body))
        .map_err(|e| bad(format!("Cannot save the uploaded file: {e}")))?;
    Ok(Json(path.to_string_lossy().into_owned()))
}

/// A file name that is safe to create on Windows, from what the browser sent.
fn safe_file_name(name: &str) -> String {
    let base = name.rsplit(['/', '\\']).next().unwrap_or_default();
    let cleaned: String = base
        .chars()
        .map(|c| match c {
            '<' | '>' | ':' | '"' | '|' | '?' | '*' => '-',
            c if c.is_control() => '-',
            c => c,
        })
        .take(120)
        .collect();
    let cleaned = cleaned.trim().trim_matches('.');
    if cleaned.is_empty() {
        "photo.jpg".to_owned()
    } else {
        cleaned.to_owned()
    }
}

#[derive(Deserialize)]
struct FileQuery {
    path: String,
}

/// Sends a photo or generated image to the browser. Only files inside the
/// data folder can be read.
async fn file(State(app): State<Ctx>, Query(query): Query<FileQuery>) -> Result<Response, Failure> {
    let not_found = || Failure(StatusCode::NOT_FOUND, "That image no longer exists.".into());
    let path = std::fs::canonicalize(&query.path).map_err(|_| not_found())?;
    if !path.starts_with(&app.data_dir) {
        return Err(Failure(
            StatusCode::FORBIDDEN,
            "That file is not part of Jewelry Studio.".into(),
        ));
    }
    let bytes = tokio::fs::read(&path).await.map_err(|_| not_found())?;
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static(image_type(&path)),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    Ok(response)
}

fn image_type(path: &Path) -> &'static str {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    match extension.as_str() {
        "jpg" | "jpeg" => "image/jpeg",
        "png" => "image/png",
        "webp" => "image/webp",
        "gif" => "image/gif",
        _ => "application/octet-stream",
    }
}

/// Generation progress, pushed to the page as it happens.
async fn events(State(app): State<Ctx>) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let stream = BroadcastStream::new(app.progress.subscribe())
        .filter_map(|received| {
            let progress = received.ok()?;
            Event::default()
                .event("generation-progress")
                .json_data(&progress)
                .ok()
        })
        .map(Ok::<_, Infallible>);
    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}

#[derive(Deserialize)]
struct ExportRequest {
    selections: Vec<OutputRef>,
    options: ExportOptions,
}

/// Renders the chosen images and sends them as a download: the image itself
/// for one, a ZIP file for several.
async fn export(
    State(app): State<Ctx>,
    Json(request): Json<ExportRequest>,
) -> Result<Response, Failure> {
    let files = service::export_files(&app.studio, request.selections, request.options)
        .await
        .map_err(bad)?;
    let count = files.len();
    let (name, content_type, bytes) = if count == 1 {
        let (name, bytes) = files.into_iter().next().expect("one file");
        let content_type = image_type(Path::new(&name));
        (name, content_type, bytes)
    } else {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        // The images are already compressed; storing them is fastest.
        let options = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Stored);
        for (name, bytes) in &files {
            zip.start_file(name.as_str(), options)
                .and_then(|()| zip.write_all(bytes).map_err(Into::into))
                .map_err(|e| bad(format!("Could not build the ZIP file: {e}")))?;
        }
        let bytes = zip
            .finish()
            .map_err(|e| bad(format!("Could not build the ZIP file: {e}")))?
            .into_inner();
        (
            "jewelry-studio-export.zip".to_owned(),
            "application/zip",
            bytes,
        )
    };

    // The page saves the file itself, so the name travels in a plain header:
    // an "attachment" header makes browsers swallow ZIP files sent to fetch().
    let mut response = Response::new(Body::from(bytes));
    let headers = response.headers_mut();
    headers.insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    if let Ok(value) = HeaderValue::from_str(&percent_encode(&name)) {
        headers.insert("x-export-name", value);
    }
    headers.insert("x-export-count", HeaderValue::from(count));
    Ok(response)
}

/// Header values must be plain ASCII; file names may not be.
fn percent_encode(text: &str) -> String {
    text.bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (byte as char).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

async fn ping() -> &'static str {
    "jewelry-studio"
}

// --- The interface -------------------------------------------------------

async fn interface(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    // Anything that is not a file of the interface gets the page itself.
    let (path, asset) = match Interface::get(path).filter(|_| !path.is_empty()) {
        Some(asset) => (path, asset),
        None => match Interface::get("index.html") {
            Some(page) => ("index.html", page),
            None => {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "The interface is missing from this program. Build it with `npm run build` in the app folder, then rebuild.",
                )
                    .into_response()
            }
        },
    };

    let mut body = asset.data.into_owned();
    if path == "index.html" {
        let page =
            String::from_utf8_lossy(&body).replacen("<head>", &format!("<head>{PAGE_MARKER}"), 1);
        body = page.into_bytes();
    }
    let mut response = Response::new(Body::from(body));
    let headers = response.headers_mut();
    if let Ok(value) = HeaderValue::from_str(asset.metadata.mimetype()) {
        headers.insert(header::CONTENT_TYPE, value);
    }
    // File names under assets/ change with every build, so they can be kept.
    let cache = if path.starts_with("assets/") {
        "public, max-age=31536000, immutable"
    } else {
        "no-cache"
    };
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    response
}

/// Refuses requests that did not come from this computer's own page: the
/// address must be this server's, and changes need the extra header.
async fn guard(State(app): State<Ctx>, request: Request, next: Next) -> Response {
    let host = request
        .headers()
        .get(header::HOST)
        .and_then(|value| value.to_str().ok())
        .unwrap_or_default();
    if !app.allowed_hosts.iter().any(|allowed| allowed == host) {
        return (
            StatusCode::FORBIDDEN,
            "Open Jewelry Studio from the address shown in its window.",
        )
            .into_response();
    }
    if request.method() != Method::GET && !request.headers().contains_key(REQUEST_HEADER) {
        return (
            StatusCode::FORBIDDEN,
            "This request did not come from Jewelry Studio.",
        )
            .into_response();
    }
    next.run(request).await
}

fn router(app: Ctx) -> Router {
    Router::new()
        .route("/api/ping", get(ping))
        .route("/api/invoke/{command}", post(invoke))
        .route("/api/upload", post(upload))
        .route("/api/file", get(file))
        .route("/api/events", get(events))
        .route("/api/export", post(export))
        .fallback(interface)
        .layer(DefaultBodyLimit::max(MAX_UPLOAD_BYTES))
        .layer(middleware::from_fn_with_state(app.clone(), guard))
        .with_state(app)
}

// --- Starting up ----------------------------------------------------------

/// The data folder: `data` next to the program, unless JEWELRY_STUDIO_DATA_DIR says otherwise.
fn data_folder() -> Result<PathBuf, String> {
    if let Some(dir) = std::env::var_os("JEWELRY_STUDIO_DATA_DIR") {
        return Ok(PathBuf::from(dir));
    }
    let exe =
        std::env::current_exe().map_err(|e| format!("Cannot find the program's folder: {e}"))?;
    Ok(exe.parent().unwrap_or(Path::new(".")).join("data"))
}

/// On the very first start, brings over the styles from the desktop app, so
/// both versions begin with the same library.
fn copy_desktop_styles(data_dir: &Path) -> Option<usize> {
    if data_dir.join("library").exists() {
        return None;
    }
    let desktop =
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?).join("com.idearigs.jewelrystudio");
    let source = desktop.join("library").join("styles");
    let entries = std::fs::read_dir(&source).ok()?;
    let target = data_dir.join("library").join("styles");
    std::fs::create_dir_all(&target).ok()?;
    let mut copied = 0;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && std::fs::copy(&path, target.join(entry.file_name())).is_ok() {
            copied += 1;
        }
    }
    // Remembers which starter styles were already given out (and maybe deleted).
    let _ = std::fs::copy(
        desktop.join("starter-styles.json"),
        data_dir.join("starter-styles.json"),
    );
    Some(copied)
}

/// Whether Jewelry Studio is already running on `port`.
fn already_running(port: u16) -> bool {
    let address = SocketAddr::from((Ipv4Addr::LOCALHOST, port));
    let Ok(mut stream) = TcpStream::connect_timeout(&address, Duration::from_millis(500)) else {
        return false;
    };
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let request = format!("GET /api/ping HTTP/1.0\r\nHost: 127.0.0.1:{port}\r\n\r\n");
    let mut reply = String::new();
    stream.write_all(request.as_bytes()).is_ok()
        && stream.read_to_string(&mut reply).is_ok()
        && reply.contains("jewelry-studio")
}

fn open_browser(url: &str) {
    if std::env::var_os("JEWELRY_STUDIO_NO_BROWSER").is_some() {
        return;
    }
    let _ = std::process::Command::new("cmd")
        .args(["/C", "start", "", url])
        .spawn();
}

fn fail(message: &str) -> ! {
    eprintln!("\nJewelry Studio could not start: {message}\n");
    eprintln!("Press Enter to close this window.");
    let _ = std::io::stdin().read_line(&mut String::new());
    std::process::exit(1);
}

#[tokio::main]
async fn main() {
    let port = std::env::var("JEWELRY_STUDIO_PORT")
        .ok()
        .and_then(|port| port.parse().ok())
        .unwrap_or(PREFERRED_PORT);
    let listener = match StdListener::bind((Ipv4Addr::LOCALHOST, port)) {
        Ok(listener) => listener,
        Err(_) if already_running(port) => {
            let url = format!("http://127.0.0.1:{port}/");
            println!("Jewelry Studio is already running. Opening {url}");
            open_browser(&url);
            return;
        }
        // Something else uses the port: let Windows pick a free one.
        Err(_) => {
            StdListener::bind((Ipv4Addr::LOCALHOST, 0)).unwrap_or_else(|e| fail(&e.to_string()))
        }
    };
    let port = listener.local_addr().map(|a| a.port()).unwrap_or(port);
    let _ = listener.set_nonblocking(true);

    let data_dir = data_folder().unwrap_or_else(|e| fail(&e));
    std::fs::create_dir_all(&data_dir)
        .unwrap_or_else(|e| fail(&format!("cannot create {}: {e}", data_dir.display())));
    let copied = copy_desktop_styles(&data_dir);
    let studio = Studio::open(data_dir.clone()).unwrap_or_else(|e| fail(&e));
    let shown_dir = data_dir.clone();
    let data_dir = std::fs::canonicalize(&data_dir).unwrap_or(data_dir);

    // Uploads are only needed until they are added; start each run clean.
    let uploads = data_dir.join("uploads");
    let _ = std::fs::remove_dir_all(&uploads);

    let (progress, _) = broadcast::channel(256);
    let app = Arc::new(App {
        studio: Mutex::new(studio),
        data_dir: data_dir.clone(),
        uploads,
        progress,
        allowed_hosts: vec![format!("127.0.0.1:{port}"), format!("localhost:{port}")],
    });

    let listener =
        tokio::net::TcpListener::from_std(listener).unwrap_or_else(|e| fail(&e.to_string()));
    let url = format!("http://127.0.0.1:{port}/");
    println!("Jewelry Studio {} (web version)", env!("CARGO_PKG_VERSION"));
    println!();
    println!("  Open in your browser:  {url}");
    println!(
        "  Your photos and results are kept in:  {}",
        shown_dir.display()
    );
    if let Some(count) = copied.filter(|count| *count > 0) {
        println!("  Copied {count} style file(s) from the desktop app.");
    }
    println!();
    println!("Only this computer can open it. Close this window to stop Jewelry Studio.");
    open_browser(&url);

    if let Err(error) = axum::serve(listener, router(app)).await {
        fail(&error.to_string());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uploaded_file_names_are_made_safe() {
        assert_eq!(safe_file_name("ring 01.jpg"), "ring 01.jpg");
        assert_eq!(safe_file_name(r"C:\fake\..\evil.png"), "evil.png");
        assert_eq!(safe_file_name("../../a:b*c?.jpg"), "a-b-c-.jpg");
        assert_eq!(safe_file_name("..."), "photo.jpg");
        assert_eq!(safe_file_name(""), "photo.jpg");
    }

    #[test]
    fn file_names_are_encoded_for_headers() {
        assert_eq!(percent_encode("ring-1.jpg"), "ring-1.jpg");
        assert_eq!(percent_encode("bague é 2.png"), "bague%20%C3%A9%202.png");
    }

    #[test]
    fn image_types_follow_the_extension() {
        assert_eq!(image_type(Path::new("a.JPG")), "image/jpeg");
        assert_eq!(image_type(Path::new("a.webp")), "image/webp");
        assert_eq!(image_type(Path::new("a.toml")), "application/octet-stream");
    }
}
