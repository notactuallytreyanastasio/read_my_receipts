use axum::{
    body::Bytes,
    extract::{DefaultBodyLimit, Multipart, Query, State},
    http::StatusCode,
    response::{Html, IntoResponse},
    routing::{get, post},
    Router,
};
use serde::Deserialize;
use tokio::sync::mpsc;

#[derive(Debug, Clone)]
pub enum PrintPayload {
    Image(Vec<u8>),
    /// Image printed without cutting — for photo strip sequences.
    /// Second field is feed lines after the image.
    ImageNoCut(Vec<u8>, u8),
    Text { text: String, source: String },
}

#[derive(Clone)]
pub struct UploadState {
    pub tx: mpsc::Sender<PrintPayload>,
}

/// GET / — mobile upload page
async fn index() -> Html<&'static str> {
    Html(UPLOAD_PAGE)
}

/// POST /print/upload — accept multipart form with "image" field
async fn upload(State(state): State<UploadState>, mut multipart: Multipart) -> impl IntoResponse {
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            let bytes = match field.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    return (StatusCode::BAD_REQUEST, format!("Read error: {e}"));
                }
            };
            if bytes.is_empty() {
                return (StatusCode::BAD_REQUEST, "Empty file".to_string());
            }
            tracing::info!("Upload received: {} bytes", bytes.len());
            if state.tx.send(PrintPayload::Image(bytes)).await.is_err() {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Print queue closed".to_string(),
                );
            }
            return (StatusCode::OK, "Queued for printing".to_string());
        }
    }
    (StatusCode::BAD_REQUEST, "No 'image' field found".to_string())
}

#[derive(Deserialize)]
struct TextParams {
    source: Option<String>,
}

#[derive(Deserialize)]
struct StripParams {
    feed: Option<u8>,
}

/// POST /print/text?source=phx.server — accept plain text body and queue for printing.
/// The `source` query param controls severity filtering:
///   - phx.server / elixir / mix: only [error] blocks are printed
///   - everything else (or omitted): all text is printed
async fn print_text(
    State(state): State<UploadState>,
    Query(params): Query<TextParams>,
    body: Bytes,
) -> impl IntoResponse {
    let text = match String::from_utf8(body.to_vec()) {
        Ok(t) => t,
        Err(_) => return (StatusCode::BAD_REQUEST, "Invalid UTF-8".to_string()),
    };
    if text.trim().is_empty() {
        return (StatusCode::BAD_REQUEST, "Empty text".to_string());
    }

    let source = params.source.unwrap_or_else(|| "shell".to_string());
    let filtered = filter_by_source(&text, &source);

    if filtered.trim().is_empty() {
        return (StatusCode::OK, "Filtered (no errors)".to_string());
    }

    tracing::info!(
        "Text print received: {} bytes (source={}, filtered from {})",
        filtered.len(),
        source,
        text.len()
    );
    if state
        .tx
        .send(PrintPayload::Text {
            text: filtered,
            source,
        })
        .await
        .is_err()
    {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "Print queue closed".to_string(),
        );
    }
    (StatusCode::OK, "Queued for printing".to_string())
}

/// Filter text based on the source program's log format.
fn filter_by_source(text: &str, source: &str) -> String {
    match source {
        "phx.server" | "elixir" | "mix" => filter_elixir_errors(text),
        _ => text.to_string(),
    }
}

/// Keep only error blocks from Elixir/Phoenix output.
/// Matches both bracketed `[error]` and bare `error:` prefixes.
/// An error block continues until the next log level marker.
fn filter_elixir_errors(text: &str) -> String {
    let mut result = Vec::new();
    let mut in_error_block = false;

    for line in text.lines() {
        if line.contains("[error]") || line.contains("error:") {
            in_error_block = true;
            result.push(line);
        } else if line.contains("[info]")
            || line.contains("[debug]")
            || line.contains("[warning]")
            || line.contains("[notice]")
            || line.contains("info:")
            || line.contains("debug:")
            || line.contains("warning:")
            || line.contains("notice:")
        {
            in_error_block = false;
        } else if in_error_block {
            // Continuation line (stacktrace, etc.)
            result.push(line);
        }
    }

    result.join("\n")
}

/// POST /print/strip — accept multipart image and print WITHOUT cutting.
/// Used by the photo booth to print a strip of photos.
/// Optional query param: ?feed=N (default 3, lines of feed after image)
async fn upload_strip(
    State(state): State<UploadState>,
    Query(params): Query<StripParams>,
    mut multipart: Multipart,
) -> impl IntoResponse {
    let feed = params.feed.unwrap_or(3);
    while let Ok(Some(field)) = multipart.next_field().await {
        let name = field.name().unwrap_or("").to_string();
        if name == "image" {
            let bytes = match field.bytes().await {
                Ok(b) => b.to_vec(),
                Err(e) => {
                    return (StatusCode::BAD_REQUEST, format!("Read error: {e}"));
                }
            };
            if bytes.is_empty() {
                return (StatusCode::BAD_REQUEST, "Empty file".to_string());
            }
            tracing::info!("Strip photo received: {} bytes", bytes.len());
            if state
                .tx
                .send(PrintPayload::ImageNoCut(bytes, feed))
                .await
                .is_err()
            {
                return (
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "Print queue closed".to_string(),
                );
            }
            return (StatusCode::OK, "Queued (no cut)".to_string());
        }
    }
    (StatusCode::BAD_REQUEST, "No 'image' field found".to_string())
}

/// POST /booth/preview — start camera preview on the display.
async fn booth_preview() -> impl IntoResponse {
    // Kill existing preview, wait, start new one, raise window — all in a background thread.
    std::thread::spawn(|| {
        let _ = std::process::Command::new("pkill")
            .args(["-f", "rpicam-hello"])
            .status();
        std::thread::sleep(std::time::Duration::from_millis(500));
        let _ = std::process::Command::new("rpicam-hello")
            .args([
                "-t", "0",
                "--viewfinder-mode", "1332:990:10:P",
            ])
            .env("DISPLAY", ":0")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    });

    (StatusCode::OK, "Preview starting".to_string())
}

/// POST /booth/shoot — run the photo booth sequence (countdown → 1 photo → print).
/// Spawns the booth binary as a detached process and returns immediately.
async fn booth_shoot() -> impl IntoResponse {
    // Find the booth binary next to this binary
    let booth_path = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|p| p.join("booth")))
        .unwrap_or_else(|| std::path::PathBuf::from("booth"));

    if !booth_path.exists() {
        return (
            StatusCode::INTERNAL_SERVER_ERROR,
            "booth binary not found".to_string(),
        );
    }

    let result = std::process::Command::new("setsid")
        .args([booth_path.to_str().unwrap()])
        .env("DISPLAY", ":0")
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();

    match result {
        Ok(_) => (StatusCode::OK, "Booth sequence started".to_string()),
        Err(e) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            format!("Failed to start booth: {e}"),
        ),
    }
}

/// GET /booth — photo booth control page
async fn booth_page() -> Html<&'static str> {
    Html(BOOTH_PAGE)
}

/// GET /admin — ops dashboard
async fn admin_page() -> Html<&'static str> {
    Html(ADMIN_PAGE)
}

/// POST /admin/run — execute a predefined ops command and return output
async fn admin_run(Query(params): Query<AdminParams>) -> impl IntoResponse {
    let (cmd, args): (&str, Vec<&str>) = match params.cmd.as_str() {
        "status" => ("make", vec!["-C", "/home/pi/read_my_receipts", "status"]),
        "restart" => ("make", vec!["-C", "/home/pi/read_my_receipts", "restart"]),
        "stop" => ("make", vec!["-C", "/home/pi/read_my_receipts", "stop"]),
        "start" => ("make", vec!["-C", "/home/pi/read_my_receipts", "start"]),
        "logs" => ("tail", vec!["-100", "/tmp/receipts.log"]),
        "test-print" => ("make", vec!["-C", "/home/pi/read_my_receipts", "test-print"]),
        "preview" => ("make", vec!["-C", "/home/pi/read_my_receipts", "preview"]),
        "preview-stop" => ("make", vec!["-C", "/home/pi/read_my_receipts", "preview-stop"]),
        "deploy" => ("make", vec!["-C", "/home/pi/read_my_receipts", "deploy"]),
        "wake-display" => {
            let _ = std::process::Command::new("xset")
                .args(["dpms", "force", "on"])
                .env("DISPLAY", ":0")
                .status();
            return (StatusCode::OK, "Display wake sent".to_string());
        }
        _ => return (StatusCode::BAD_REQUEST, format!("Unknown command: {}", params.cmd)),
    };

    let result = std::process::Command::new(cmd)
        .args(&args)
        .env("DISPLAY", ":0")
        .output();

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout);
            let stderr = String::from_utf8_lossy(&output.stderr);
            // Strip ANSI escape codes
            let clean = format!("{}{}", stdout, stderr)
                .replace("\x1b[0m", "")
                .replace("\x1b[2m", "")
                .replace("\x1b[32m", "")
                .replace("\x1b[33m", "")
                .replace("\x1b[34m", "")
                .replace("\x1b[31m", "")
                .replace("\x1b[1;32m", "")
                .replace("\x1b[1;37m", "")
                .replace("\x1b[1;34m", "")
                .replace("\x1b[1;31m", "")
                .replace("\x1b[0;32m", "");
            let status = if output.status.success() { "ok" } else { "error" };
            (StatusCode::OK, format!("[{}]\n{}", status, clean))
        }
        Err(e) => (StatusCode::INTERNAL_SERVER_ERROR, format!("Failed: {e}")),
    }
}

#[derive(Deserialize)]
struct AdminParams {
    cmd: String,
}

/// iOS captive portal check: return "Success" so iOS thinks the network
/// has internet and dismisses the captive portal mini-browser.
/// The user then opens Safari to http://192.168.4.1 for the real page.
async fn captive_success() -> Html<&'static str> {
    Html("<HTML><HEAD><TITLE>Success</TITLE></HEAD><BODY>Success</BODY></HTML>")
}

/// Android captive portal check
async fn generate_204() -> StatusCode {
    StatusCode::NO_CONTENT
}

pub fn build_router(tx: mpsc::Sender<PrintPayload>) -> Router {
    let state = UploadState { tx };
    Router::new()
        .route("/", get(index))
        .route("/print/upload", post(upload))
        .route("/print/strip", post(upload_strip))
        .route("/print/text", post(print_text))
        .route("/booth/preview", post(booth_preview))
        .route("/booth/shoot", post(booth_shoot))
        .route("/booth", get(booth_page))
        .route("/admin", get(admin_page))
        .route("/admin/run", post(admin_run))
        .route("/hotspot-detect.html", get(captive_success))
        .route("/library/test/success.html", get(captive_success))
        .route("/generate_204", get(generate_204))
        .fallback(get(index))
        .layer(DefaultBodyLimit::max(15 * 1024 * 1024))
        .with_state(state)
}

const UPLOAD_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no">
<title>Print Photo</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#111;color:#fff;min-height:100vh;display:flex;align-items:center;justify-content:center}
.wrap{width:100%;max-width:400px;padding:20px}
h1{font-size:28px;text-align:center;margin-bottom:6px}
.sub{color:#888;text-align:center;font-size:13px;margin-bottom:32px}
.pick{display:block;width:100%;padding:50px 20px;background:#1a1a1a;border:2px dashed #333;border-radius:12px;text-align:center;cursor:pointer;transition:border-color .2s;margin-bottom:16px}
.pick:active{border-color:#666}
.pick.has{border-color:#4a9;border-style:solid}
.pick input{display:none}
.pick-text{color:#888;font-size:15px}
.preview{margin-bottom:16px;text-align:center}
.preview img{max-width:100%;max-height:280px;border-radius:8px}
.btn{display:block;width:100%;padding:16px;background:#fff;color:#000;border:none;border-radius:10px;font-size:17px;font-weight:600;cursor:pointer}
.btn:disabled{opacity:.3;cursor:not-allowed}
.status{margin-top:16px;padding:12px;border-radius:8px;font-size:14px;text-align:center;display:none}
.status.ok{background:#1a3a2a;color:#4a9;display:block}
.status.err{background:#3a1a1a;color:#e55;display:block}
.status.wait{background:#1a2a3a;color:#5ae;display:block}
.again{display:none;margin-top:12px;text-align:center}
.again a{color:#5ae;font-size:14px;text-decoration:none}
</style>
</head>
<body>
<div class="wrap">
<h1>Print Photo</h1>
<p class="sub">Take a photo or pick one to print</p>

<label class="pick" id="pick">
<input type="file" id="file" accept="image/*" multiple>
<span class="pick-text" id="pick-text">Tap to select photos</span>
</label>

<div class="preview" id="preview"></div>
<button class="btn" id="btn" disabled>Print</button>
<div id="status" class="status"></div>
<div class="again" id="again"><a href="/">Print more</a></div>
<div style="margin-top:32px;text-align:center"><a href="/booth" style="color:#5ae;font-size:15px;text-decoration:none">Photo Booth Mode</a></div>
</div>

<script>
const file=document.getElementById('file'),
  pick=document.getElementById('pick'),
  btn=document.getElementById('btn'),
  preview=document.getElementById('preview'),
  status=document.getElementById('status'),
  again=document.getElementById('again');

let files=[];

file.addEventListener('change',function(){
  files=Array.from(this.files);
  if(!files.length)return;
  const n=files.length;
  document.getElementById('pick-text').textContent=n+' photo'+(n>1?'s':'')+' selected';
  pick.classList.add('has');
  btn.disabled=false;
  preview.innerHTML=files.map((_,i)=>{
    const r=new FileReader();
    r.onload=e=>{
      const el=document.getElementById('thumb'+i);
      if(el)el.src=e.target.result;
    };
    r.readAsDataURL(files[i]);
    return '<img id="thumb'+i+'" style="max-width:80px;max-height:80px;border-radius:6px;margin:4px">';
  }).join('');
  status.className='status';
  again.style.display='none';
});

const sleep=ms=>new Promise(r=>setTimeout(r,ms));

btn.addEventListener('click',async()=>{
  if(!files.length)return;
  btn.disabled=true;
  const total=files.length;
  let ok=0,fail=0;
  for(let i=0;i<total;i++){
    btn.textContent='Sending '+(i+1)+'/'+total+'...';
    status.className='status wait';
    status.textContent='Uploading photo '+(i+1)+' of '+total+'...';
    if(i>0)await sleep(2000);
    const fd=new FormData();
    fd.append('image',files[i]);
    try{
      const resp=await fetch('/print/upload',{method:'POST',body:fd});
      if(resp.ok){ok++}else{fail++}
    }catch(e){fail++}
  }
  if(fail===0){
    status.className='status ok';
    status.textContent='All '+ok+' photo'+(ok>1?'s':'')+' sent to printer!';
  }else{
    status.className='status err';
    status.textContent=ok+' sent, '+fail+' failed';
  }
  again.style.display='block';
  btn.disabled=false;
  btn.textContent='Print';
});
</script>
</body>
</html>"#;

const BOOTH_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0, user-scalable=no">
<title>Photo Booth</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,sans-serif;background:#111;color:#fff;min-height:100vh;display:flex;align-items:center;justify-content:center}
.wrap{width:100%;max-width:400px;padding:20px;text-align:center}
h1{font-size:28px;margin-bottom:6px}
.sub{color:#888;font-size:13px;margin-bottom:32px}
.btn{display:block;width:100%;padding:16px;border:none;border-radius:10px;font-size:17px;font-weight:600;cursor:pointer;margin-bottom:12px}
.btn:disabled{opacity:.3;cursor:not-allowed}
.btn-preview{background:#333;color:#fff}
.btn-shoot{background:#fff;color:#000}
.status{margin-top:16px;padding:12px;border-radius:8px;font-size:14px;text-align:center;display:none}
.status.ok{background:#1a3a2a;color:#4a9;display:block}
.status.err{background:#3a1a1a;color:#e55;display:block}
.status.wait{background:#1a2a3a;color:#5ae;display:block}
</style>
</head>
<body>
<div class="wrap">
<h1>Photo Booth</h1>
<p class="sub">3 photos, printed as a strip</p>

<button class="btn btn-preview" id="preview-btn">Start Preview</button>
<button class="btn btn-shoot" id="shoot-btn" disabled>Take Photos</button>
<div id="status" class="status"></div>
</div>

<script>
const previewBtn=document.getElementById('preview-btn'),
  shootBtn=document.getElementById('shoot-btn'),
  status=document.getElementById('status');

let shooting=false;

previewBtn.addEventListener('click',async()=>{
  previewBtn.disabled=true;
  previewBtn.textContent='Starting...';
  status.className='status wait';
  status.textContent='Starting camera preview on screen...';
  try{
    const r=await fetch('/booth/preview',{method:'POST'});
    if(r.ok){
      previewBtn.textContent='Preview Running';
      shootBtn.disabled=false;
      status.className='status ok';
      status.textContent='Position yourself and hit Take Photos!';
    }else{
      const t=await r.text();
      status.className='status err';
      status.textContent='Error: '+t;
      previewBtn.disabled=false;
      previewBtn.textContent='Start Preview';
    }
  }catch(e){
    status.className='status err';
    status.textContent='Connection failed';
    previewBtn.disabled=false;
    previewBtn.textContent='Start Preview';
  }
});

shootBtn.addEventListener('click',async()=>{
  if(shooting)return;
  shooting=true;
  shootBtn.disabled=true;
  shootBtn.textContent='Shooting...';
  status.className='status wait';
  status.textContent='Get ready! Countdown starting on screen...';
  try{
    const r=await fetch('/booth/shoot',{method:'POST'});
    if(r.ok){
      status.className='status wait';
      status.textContent='Photos being taken and printed... hold tight!';
      setTimeout(()=>{
        status.className='status ok';
        status.textContent='Strip printed! Check the printer.';
        shootBtn.textContent='Take Photos';
        shootBtn.disabled=false;
        previewBtn.disabled=false;
        previewBtn.textContent='Start Preview';
        shooting=false;
      },45000);
    }else{
      const t=await r.text();
      status.className='status err';
      status.textContent='Error: '+t;
      shootBtn.disabled=false;
      shootBtn.textContent='Take Photos';
      shooting=false;
    }
  }catch(e){
    status.className='status err';
    status.textContent='Connection failed';
    shootBtn.disabled=false;
    shootBtn.textContent='Take Photos';
    shooting=false;
  }
});
</script>
</body>
</html>"#;

const ADMIN_PAGE: &str = r#"<!DOCTYPE html>
<html lang="en">
<head>
<meta charset="UTF-8">
<meta name="viewport" content="width=device-width, initial-scale=1.0">
<title>Admin</title>
<style>
*{margin:0;padding:0;box-sizing:border-box}
body{font-family:-apple-system,BlinkMacSystemFont,monospace;background:#0a0a0a;color:#ccc;padding:16px;max-width:600px;margin:0 auto}
h1{font-size:20px;margin-bottom:4px;color:#fff}
.sub{color:#666;font-size:12px;margin-bottom:20px}
.section{margin-bottom:20px}
.section h2{font-size:13px;color:#888;text-transform:uppercase;letter-spacing:1px;margin-bottom:8px;border-bottom:1px solid #222;padding-bottom:4px}
.grid{display:grid;grid-template-columns:1fr 1fr;gap:8px}
.btn{padding:12px 8px;border:1px solid #333;border-radius:6px;background:#1a1a1a;color:#fff;font-size:14px;font-family:inherit;cursor:pointer;text-align:center;transition:all .15s}
.btn:hover{background:#252525;border-color:#555}
.btn:active{background:#333}
.btn:disabled{opacity:.3;cursor:wait}
.btn-danger{border-color:#522;color:#e55}
.btn-danger:hover{background:#2a1515}
.btn-success{border-color:#253;color:#4a9}
.btn-success:hover{background:#152a1a}
.btn-wide{grid-column:1/-1}
#output{margin-top:16px;background:#111;border:1px solid #222;border-radius:6px;padding:12px;font-size:12px;line-height:1.5;white-space:pre-wrap;word-break:break-all;max-height:400px;overflow-y:auto;display:none}
.nav{margin-bottom:16px;font-size:13px}
.nav a{color:#5ae;text-decoration:none;margin-right:12px}
</style>
</head>
<body>
<h1>Admin Panel</h1>
<p class="sub">Ops commands for Read My Receipts</p>
<div class="nav"><a href="/">Upload</a><a href="/booth">Booth</a><a href="/admin">Admin</a></div>

<div class="section">
<h2>Diagnostics</h2>
<div class="grid">
<button class="btn btn-wide" onclick="run('status')">Status</button>
<button class="btn" onclick="run('logs')">Recent Logs</button>
<button class="btn" onclick="run('test-print')">Test Print</button>
</div>
</div>

<div class="section">
<h2>Process</h2>
<div class="grid">
<button class="btn btn-success" onclick="run('restart')">Restart</button>
<button class="btn btn-danger" onclick="run('stop')">Stop</button>
<button class="btn btn-success" onclick="run('start')">Start</button>
<button class="btn" onclick="run('deploy')">Build &amp; Deploy</button>
</div>
</div>

<div class="section">
<h2>Camera</h2>
<div class="grid">
<button class="btn" onclick="run('preview')">Start Preview</button>
<button class="btn" onclick="run('preview-stop')">Stop Preview</button>
</div>
</div>

<div class="section">
<h2>Display</h2>
<div class="grid">
<button class="btn btn-wide" onclick="run('wake-display')">Wake Display</button>
</div>
</div>

<pre id="output"></pre>

<script>
const out=document.getElementById('output');
async function run(cmd){
  out.style.display='block';
  out.textContent='Running '+cmd+'...\n';
  document.querySelectorAll('.btn').forEach(b=>b.disabled=true);
  try{
    const r=await fetch('/admin/run?cmd='+encodeURIComponent(cmd),{method:'POST'});
    const t=await r.text();
    out.textContent=t;
    out.scrollTop=out.scrollHeight;
  }catch(e){
    out.textContent='Connection failed: '+e;
  }
  document.querySelectorAll('.btn').forEach(b=>b.disabled=false);
}
</script>
</body>
</html>"#;
