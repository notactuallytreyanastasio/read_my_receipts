use axum::{
    extract::{DefaultBodyLimit, Multipart, State},
    http::StatusCode,
    response::{Html, IntoResponse, Redirect},
    routing::{get, post},
    Router,
};
use tokio::sync::mpsc;

#[derive(Clone)]
pub struct UploadState {
    pub tx: mpsc::Sender<Vec<u8>>,
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
            if state.tx.send(bytes).await.is_err() {
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

/// Captive portal: return redirect to / so iOS shows the popup
async fn captive_redirect() -> Redirect {
    Redirect::temporary("/")
}

pub fn build_router(tx: mpsc::Sender<Vec<u8>>) -> Router {
    let state = UploadState { tx };
    Router::new()
        .route("/", get(index))
        .route("/print/upload", post(upload))
        .route("/hotspot-detect.html", get(captive_redirect))
        .fallback(get(captive_redirect))
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
<input type="file" id="file" accept="image/*" capture="environment">
<span class="pick-text" id="pick-text">Tap to open camera</span>
</label>

<div class="preview" id="preview"></div>
<button class="btn" id="btn" disabled>Print</button>
<div id="status" class="status"></div>
<div class="again" id="again"><a href="/">Print another</a></div>
</div>

<script>
const file=document.getElementById('file'),
  pick=document.getElementById('pick'),
  btn=document.getElementById('btn'),
  preview=document.getElementById('preview'),
  status=document.getElementById('status'),
  again=document.getElementById('again');

let selected=null;

file.addEventListener('change',function(){
  selected=this.files[0];
  if(!selected)return;
  document.getElementById('pick-text').textContent=selected.name;
  pick.classList.add('has');
  btn.disabled=false;
  const r=new FileReader();
  r.onload=e=>{preview.innerHTML='<img src="'+e.target.result+'">'};
  r.readAsDataURL(selected);
  status.className='status';
  again.style.display='none';
});

btn.addEventListener('click',async()=>{
  if(!selected)return;
  btn.disabled=true;
  btn.textContent='Sending...';
  status.className='status wait';
  status.textContent='Uploading photo...';
  const fd=new FormData();
  fd.append('image',selected);
  try{
    const resp=await fetch('/print/upload',{method:'POST',body:fd});
    if(resp.ok){
      status.className='status ok';
      status.textContent='Sent to printer!';
      again.style.display='block';
    }else{
      const t=await resp.text();
      status.className='status err';
      status.textContent='Error: '+t;
    }
  }catch(e){
    status.className='status err';
    status.textContent='Connection failed';
  }
  btn.disabled=false;
  btn.textContent='Print';
});
</script>
</body>
</html>"#;
