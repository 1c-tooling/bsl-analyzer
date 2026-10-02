//! Pilot-only raw/enriched A/B input: reuse the product serializer and persisted graph context.
use std::{collections::HashMap, error::Error, fs, path::Path};

fn main() -> Result<(), Box<dyn Error>> {
    let args: Vec<_> = std::env::args().collect();
    if args.len() != 4 {
        return Err("usage: export_embedding_inputs STORE FROZEN_QRELS OUTPUT".into());
    }
    let store = bsl_search::Store::open_reader(Path::new(&args[1]))?;
    let documents = store.load_indexed_documents(Some("code"))?;
    let mut by_id: HashMap<_, _> = documents
        .into_iter()
        .map(|doc| {
            let root = if doc.root_id.is_empty() { "cf" } else { &doc.root_id };
            (format!("{root}/{}::{}", doc.path, doc.symbol_name), doc)
        })
        .collect();
    let mut bundle: serde_json::Value = serde_json::from_slice(&fs::read(&args[2])?)?;
    let corpus = bundle["corpus"].as_object_mut().ok_or("missing corpus")?;
    for (id, entry) in corpus {
        let mut document = by_id.remove(id).ok_or("frozen method absent from native corpus")?;
        document.text = entry["text"].as_str().ok_or("missing raw method text")?.to_owned();
        entry["text"] = bsl_search::semantic_text_for_indexed_document(&document).into();
    }
    bundle["evaluation_layout"] = "product-enriched-with-identical-frozen-raw-body".into();
    let output = fs::OpenOptions::new().write(true).create_new(true).open(&args[3])?;
    serde_json::to_writer(output, &bundle)?;
    Ok(())
}
