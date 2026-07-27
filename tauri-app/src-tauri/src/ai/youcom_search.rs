use reqwest::header::{HeaderMap, HeaderValue, CONTENT_TYPE};
use serde::{Deserialize, Serialize};

const YOUCOM_SEARCH_URL: &str = "https://ydc-index.io/v1/search";

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct YoucomSearchResult {
    pub title: String,
    pub url: String,
    pub content: String,
}

#[derive(Debug, Deserialize)]
struct YoucomResponse {
    results: Option<Vec<YoucomRawResult>>,
}

#[derive(Debug, Deserialize)]
struct YoucomRawResult {
    title: Option<String>,
    url: Option<String>,
    #[serde(default)]
    snippets: Vec<String>,
    description: Option<String>,
}

/// Perform a You.com web search.
pub async fn youcom_search(query: &str, max_results: usize) -> Result<Vec<YoucomSearchResult>, String> {
    let api_key = std::env::var("YOUCOM_API_KEY")
        .map_err(|_| "YOUCOM_API_KEY not set")?;

    if api_key.trim().is_empty() {
        return Err("YOUCOM_API_KEY is empty".to_string());
    }

    let count = max_results.clamp(1, 20);
    let client = reqwest::Client::new();

    let mut headers = HeaderMap::new();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(
        "X-API-Key",
        HeaderValue::from_str(&api_key).map_err(|e| format!("Invalid API key header: {}", e))?,
    );

    let response = client
        .post(YOUCOM_SEARCH_URL)
        .headers(headers)
        .json(&serde_json::json!({
            "query": query,
            "count": count
        }))
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|e| format!("HTTP request failed: {}", e))?;

    let status = response.status();
    if status.as_u16() == 429 {
        return Err("You.com rate limit exceeded (429)".to_string());
    }
    if status.as_u16() == 401 {
        return Err("You.com API key invalid or expired (401)".to_string());
    }
    if !status.is_success() {
        return Err(format!("You.com search failed with status {}", status));
    }

    let body: YoucomResponse = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {}", e))?;

    let results = body
        .results
        .unwrap_or_default()
        .into_iter()
        .take(count)
        .map(|item| YoucomSearchResult {
            title: item.title.unwrap_or_default(),
            url: item.url.unwrap_or_default(),
            content: if item.snippets.is_empty() {
                item.description.unwrap_or_default()
            } else {
                item.snippets[0].clone()
            },
        })
        .collect();

    Ok(results)
}

/// Format search results as a readable string for the AI.
pub fn format_search_results(results: Vec<YoucomSearchResult>) -> String {
    if results.is_empty() {
        return "No results found.".to_string();
    }

    results
        .into_iter()
        .enumerate()
        .map(|(i, r)| {
            format!(
                "[{}] {}\nURL: {}\n{}\n---",
                i + 1,
                r.title,
                r.url,
                r.content
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}