use anyhow::{Context, Result};
use dotenv::dotenv;
use google_sheets4::{
    api::{BatchUpdateValuesRequest, ValueRange},
    hyper_rustls,
    yup_oauth2::{read_service_account_key, ServiceAccountAuthenticator},
    Sheets,
};
use hyper_rustls::HttpsConnectorBuilder;
use hyper_util::{
    client::legacy::{connect::HttpConnector, Client as HyperClient},
    rt::TokioExecutor,
};
use once_cell::sync::Lazy;
use rand::Rng;
use serde_json::json;
use std::{
    collections::{HashMap, HashSet},
    env,
    sync::Arc,
};
use tokio::sync::Mutex;

type SheetsHub = Sheets<hyper_rustls::HttpsConnector<HttpConnector>>;

// Stop words: customize freely for your niche.
static STOP_WORDS: Lazy<HashSet<&'static str>> = Lazy::new(|| {
    HashSet::from([
        "a", "an", "the", "and", "or", "but", "of", "to", "in", "on", "at", "for", "from", "by",
        "with", "as", "into", "over", "under", "is", "are", "was", "were", "be", "been", "being",
        "this", "that", "these", "those", "it", "its", "his", "her", "their", "our", "your", "my",
        "i", "you", "he", "she", "we", "they", "before", "after", "during", "late", "early",
        "photo", "photos", "pic", "pics", "image", "images", "tour", "band", "group",
    ])
});

fn tokenize_ascii_words(s: &str) -> Vec<String> {
    let s = s.to_lowercase().replace('&', " and ");

    let mut tokens = Vec::new();
    let mut cur = String::new();

    for ch in s.chars() {
        if ch.is_ascii_alphanumeric() {
            cur.push(ch);
        } else if !cur.is_empty() {
            tokens.push(std::mem::take(&mut cur));
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn build_global_freq(snippets: &[String]) -> HashMap<String, u32> {
    let mut freq: HashMap<String, u32> = HashMap::new();
    for s in snippets {
        for w in tokenize_ascii_words(s) {
            if w.len() <= 1 {
                continue;
            }
            if STOP_WORDS.contains(w.as_str()) {
                continue;
            }
            *freq.entry(w).or_insert(0) += 1;
        }
    }
    freq
}

fn base_slug_from_snippet_unusual(snippet: &str, freq: &HashMap<String, u32>) -> String {
    let mut seen = HashSet::new();
    let mut candidates: Vec<(String, u32, usize)> = Vec::new(); // (word, global_freq, pos)

    for (pos, w) in tokenize_ascii_words(snippet).into_iter().enumerate() {
        if w.len() <= 1 {
            continue;
        }
        if STOP_WORDS.contains(w.as_str()) {
            continue;
        }
        if !seen.insert(w.clone()) {
            continue;
        }
        let f = *freq.get(&w).unwrap_or(&1);
        candidates.push((w, f, pos));
    }

    if candidates.is_empty() {
        return "image".to_string();
    }

    // rare first, tie-break by earlier appearance
    candidates.sort_by(|a, b| a.1.cmp(&b.1).then(a.2.cmp(&b.2)));

    // take top 5 by rarity, then re-sort to keep readable order
    let mut picked = candidates.into_iter().take(5).collect::<Vec<_>>();
    picked.sort_by_key(|x| x.2);

    picked
        .into_iter()
        .map(|x| x.0)
        .collect::<Vec<_>>()
        .join("-")
}

fn random_10_digit_string() -> String {
    // Ensures a 10-digit number (leading digit non-zero).
    let n: u64 = rand::rng().random_range(1_000_000_000u64..=9_999_999_999u64);
    n.to_string()
}

fn slug_from_snippet_or_random10(snippet: &str, freq: &HashMap<String, u32>) -> String {
    let trimmed = snippet.trim();
    if trimmed.is_empty() {
        return random_10_digit_string();
    }

    let base = base_slug_from_snippet_unusual(trimmed, freq);
    let suffix: u32 = rand::rng().random_range(1000..=9999);
    format!("{base}-{suffix}")
}

struct SheetBridge {
    spreadsheet_id: String,
    tab_name: String,
    hub: Arc<Mutex<SheetsHub>>,
}

impl SheetBridge {
    async fn new(spreadsheet_id: String, tab_name: String, credentials_path: String) -> Result<Self> {
        let service_account_key = read_service_account_key(credentials_path).await?;
               let auth = ServiceAccountAuthenticator::builder(service_account_key)
            .build()
            .await?;

        let https = HttpsConnectorBuilder::new()
            .with_native_roots()
            .expect("TLS roots")
            .https_or_http()
            .enable_http1()
            .enable_http2()
            .build();

        let client = HyperClient::builder(TokioExecutor::new()).build(https);
        let hub = Sheets::new(client, auth);

        Ok(Self {
            spreadsheet_id,
            tab_name,
            hub: Arc::new(Mutex::new(hub)),
        })
    }

    async fn read_column(&self, column_range: &str) -> Result<Vec<String>> {
        let range = format!("{}!{}", self.tab_name, column_range);
        let (_resp, vr) = self
            .hub
            .lock()
            .await
            .spreadsheets()
            .values_get(&self.spreadsheet_id, &range)
            .doit()
            .await?;

        let mut out = Vec::new();
        if let Some(rows) = vr.values {
            for row in rows {
                let val = row
                    .get(0)
                    .and_then(|v| v.as_str())
                    .unwrap_or("")
                    .to_string();
                out.push(val);
            }
        }
        Ok(out)
    }

    async fn write_to_column(&self, column: &str, values_by_row_1_based: Vec<(usize, String)>) -> Result<()> {
        let mut data: Vec<ValueRange> = Vec::with_capacity(values_by_row_1_based.len());

        for (row, value) in values_by_row_1_based {
            data.push(ValueRange {
                range: Some(format!("{}!{}{}", self.tab_name, column, row)),
                values: Some(vec![vec![json!(value)]]),
                ..Default::default()
            });
        }

        if data.is_empty() {
            return Ok(());
        }

        let request = BatchUpdateValuesRequest {
            value_input_option: Some("RAW".into()),
            data: Some(data),
            ..Default::default()
        };

        self.hub
            .lock()
            .await
            .spreadsheets()
            .values_batch_update(request, &self.spreadsheet_id)
            .doit()
            .await?;

        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    rustls::crypto::ring::default_provider()
        .install_default()
        .expect("Failed to install rustls crypto provider");

    dotenv().ok();

    let spreadsheet_id = env::var("GOOGLE_SHEET_ID").context("GOOGLE_SHEET_ID missing")?;
    let sheet_tab = env::var("GOOGLE_SHEET_TAB").unwrap_or_else(|_| "Sheet1".to_string());
    let creds_path = env::var("GOOGLE_APPLICATION_CREDENTIALS")
        .context("GOOGLE_APPLICATION_CREDENTIALS missing")?;
    let trigger_column = env::var("GOOGLE_SHEET_TRIGGER_COLUMN")
        .context("GOOGLE_SHEET_TRIGGER_COLUMN missing")?
        .to_uppercase();
    let snippet_column = env::var("GOOGLE_SHEET_SNIPPET_COLUMN")
        .context("GOOGLE_SHEET_SNIPPET_COLUMN missing")?
        .to_uppercase();
    let slug_column = env::var("GOOGLE_SHEET_SLUG_COLUMN")
        .context("GOOGLE_SHEET_SLUG_COLUMN missing")?
        .to_uppercase();

    let sheet = SheetBridge::new(spreadsheet_id, sheet_tab, creds_path).await?;

    // The trigger column gates row processing; the snippet column feeds the
    // slug generator; the slug column holds existing/generated slugs.
    let col_d = sheet.read_column(&format!("{trigger_column}:{trigger_column}")).await?;
    let col_e = sheet.read_column(&format!("{snippet_column}:{snippet_column}")).await?;
    let col_i = sheet.read_column(&format!("{slug_column}:{slug_column}")).await?;

    // Build frequency map from column E (skipping header row).
    let snippets_no_header: Vec<String> = col_e.iter().skip(1).cloned().collect();
    let freq = build_global_freq(&snippets_no_header);

    let mut updates: Vec<(usize, String)> = Vec::new();
    let mut generated_count = 0usize;
    let mut skipped_existing = 0usize;
    let mut skipped_no_trigger = 0usize;

    let row_count = col_d.len().max(col_e.len()).max(col_i.len());
    for idx0 in 0..row_count {
        let row = idx0 + 1;
        if row == 1 {
            continue; // header
        }

        let trigger = col_d.get(idx0).map(|s| s.trim()).unwrap_or("");
        if trigger.is_empty() {
            skipped_no_trigger += 1;
            continue; // nothing in the trigger column -> do not generate
        }

        let existing_slug = col_i.get(idx0).map(|s| s.trim()).unwrap_or("");
        if !existing_slug.is_empty() {
            skipped_existing += 1;
            continue; // slug already present
        }

        let snippet = col_e.get(idx0).cloned().unwrap_or_default();
        let out_value = slug_from_snippet_or_random10(&snippet, &freq);
        updates.push((row, out_value));
        generated_count += 1;
    }

    sheet.write_to_column(&slug_column, updates).await?;

    println!("Generated {generated_count} new slugs.");
    println!("Skipped {skipped_existing} rows because slugs already existed.");
    println!("Skipped {skipped_no_trigger} rows because the trigger column was empty.");

    Ok(())
}