use std::{env, error::Error, process};

use httppilot_core::{HttpEngine, HttpRequest};

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let Some(method) = args.next() else {
        usage();
    };
    let Some(url) = args.next() else {
        usage();
    };

    let mut request = HttpRequest::new(method, url);
    let body = args.collect::<Vec<_>>().join(" ");
    if !body.is_empty() {
        request.body = Some(body);
    }

    let response = HttpEngine::default().execute(request).await?;
    eprintln!(
        "HTTP {} in {}ms ({} preview bytes{})",
        response.status,
        response.elapsed.as_millis(),
        response.preview_bytes,
        if response.truncated { ", truncated" } else { "" },
    );
    println!("{}", response.body_preview);
    Ok(())
}

fn usage() -> ! {
    eprintln!("Usage: httppilot-cli METHOD URL [BODY]");
    eprintln!("Example: httppilot-cli GET https://httpbin.org/get");
    process::exit(2);
}
