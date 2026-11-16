use std::{
    env,
    error::Error,
    fs,
    io::{self, Write},
    process,
};

use httppilot_core::{HttpEngine, HttpRequest};
use httppilot_storage::{Storage, export_postman_json, import_postman_json};

const MAX_POSTMAN_IMPORT_BYTES: u64 = 10 * 1024 * 1024;

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let mut args = env::args().skip(1);
    let Some(command) = args.next() else {
        usage();
    };

    match command.as_str() {
        "list-collections" => {
            ensure_no_extra_args(args)?;
            let storage = Storage::open_default()?;
            let workspace = storage.load()?;
            for collection in &workspace.collections {
                println!(
                    "{}\t{} requests\t{}",
                    collection.id,
                    collection.requests.len(),
                    collection.name
                );
            }
        }
        "import-postman" => {
            let path = required_arg(&mut args, "Postman collection JSON file")?;
            ensure_no_extra_args(args)?;

            let metadata = fs::metadata(&path)?;
            if metadata.len() > MAX_POSTMAN_IMPORT_BYTES {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "Postman import exceeds the 10 MiB file limit",
                )
                .into());
            }
            let contents = fs::read_to_string(&path)?;
            let imported = import_postman_json(&contents)?;
            let imported_count = imported.requests.len();

            let storage = Storage::open_default()?;
            let (mut workspace, revision) = storage.load_versioned()?;
            let id = workspace.append_imported_collection(imported);
            storage.save_versioned(&workspace, revision)?;

            eprintln!(
                "Warning: request URLs, bodies and non-secret fields are stored as plaintext in SQLite. \
                 Postman auth and recognized secret headers are not imported."
            );
            println!("Imported collection {id} with {imported_count} requests.");
        }
        "export-postman" => {
            let id: u64 = required_arg(&mut args, "collection ID")?.parse()?;
            let filename = required_arg(&mut args, "new output filename")?;
            ensure_no_extra_args(args)?;

            let storage = Storage::open_default()?;
            let workspace = storage.load()?;
            let collection = workspace
                .collections
                .iter()
                .find(|collection| collection.id == id)
                .ok_or_else(|| {
                    io::Error::new(io::ErrorKind::NotFound, "collection ID does not exist")
                })?;
            let data = export_postman_json(collection)?;

            // Do not overwrite existing files; on Unix, restrict new files to
            // the current user. On Windows, inherited filesystem ACLs apply.
            let mut options = fs::OpenOptions::new();
            options.write(true).create_new(true);
            #[cfg(unix)]
            {
                use std::os::unix::fs::OpenOptionsExt;
                options.mode(0o600);
            }
            let mut output = options.open(&filename)?;
            output.write_all(data.as_bytes())?;

            eprintln!(
                "Warning: exported URLs, request bodies and ordinary fields may contain secrets. \
                 Review the JSON before sharing it."
            );
            println!("Exported collection {id} to {filename}");
        }
        _ => {
            let url = required_arg(&mut args, "request URL")?;
            let mut request = HttpRequest::new(command, url);
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
                if response.truncated {
                    ", truncated"
                } else {
                    ""
                },
            );
            println!("{}", response.body_preview);
        }
    }
    Ok(())
}

fn required_arg(args: &mut impl Iterator<Item = String>, name: &str) -> Result<String, io::Error> {
    args.next()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, format!("missing {name}")))
}

fn ensure_no_extra_args(mut args: impl Iterator<Item = String>) -> Result<(), io::Error> {
    if args.next().is_some() {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "unexpected extra arguments",
        ))
    } else {
        Ok(())
    }
}

fn usage() -> ! {
    eprintln!("HttpPilot command-line client");
    eprintln!("Usage:");
    eprintln!("  httppilot-cli METHOD URL [BODY]");
    eprintln!("  httppilot-cli list-collections");
    eprintln!("  httppilot-cli import-postman <collection-v2.1.json>");
    eprintln!("  httppilot-cli export-postman <collection-id> <new-output.json>");
    process::exit(2);
}
