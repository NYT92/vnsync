use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct VndbResult {
    pub id: String,
    pub title: String,
    pub alttitle: String,
    pub released: String,
    pub description: String,
    pub image_url: String,
    pub developer: String,
    pub rating: f64,
}

#[derive(Deserialize)]
struct VndbResponse {
    results: Vec<VndbEntry>,
}

#[derive(Deserialize)]
struct VndbEntry {
    id: String,
    title: String,
    alttitle: Option<String>,
    released: Option<String>,
    description: Option<String>,
    rating: Option<f64>,
    image: Option<VndbImage>,
    developers: Option<Vec<VndbProducer>>,
}

#[derive(Deserialize)]
struct VndbImage {
    url: Option<String>,
}

#[derive(Deserialize)]
struct VndbProducer {
    name: String,
}

pub async fn search_vndb(query: &str) -> Result<Vec<VndbResult>, String> {
    let trimmed = query.trim();
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }

    let client = reqwest::Client::new();
    let payload = serde_json::json!({
        "filters": ["search", "=", trimmed],
        "fields": "id, title, alttitle, released, description, rating, image.url, developers.name",
        "results": 10
    });

    let resp = client
        .post("https://api.vndb.org/kana/vn")
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("VNDB request failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("VNDB returned status {}", resp.status()));
    }

    let body: VndbResponse = resp
        .json()
        .await
        .map_err(|e| format!("Failed parsing VNDB response: {}", e))?;

    let results = body
        .results
        .into_iter()
        .map(|entry| {
            let dev = entry
                .developers
                .unwrap_or_default()
                .into_iter()
                .map(|d| d.name)
                .collect::<Vec<_>>()
                .join(", ");
            let clean_desc = strip_vndb_bbcode(&entry.description.unwrap_or_default());
            VndbResult {
                id: entry.id,
                title: entry.title,
                alttitle: entry.alttitle.unwrap_or_default(),
                released: entry.released.unwrap_or_default(),
                description: clean_desc,
                image_url: entry.image.and_then(|i| i.url).unwrap_or_default(),
                developer: dev,
                rating: entry.rating.unwrap_or(0.0),
            }
        })
        .collect();

    Ok(results)
}

fn strip_vndb_bbcode(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut inside_tag = false;
    for ch in input.chars() {
        match ch {
            '[' => inside_tag = true,
            ']' => inside_tag = false,
            _ if !inside_tag => out.push(ch),
            _ => {}
        }
    }
    out.trim().to_string()
}
