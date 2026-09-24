use anyhow::{ensure, Context, Result};
use reqwest::Url;
use scraper::{Html, Selector};
use serde_json::{json, Value};
use std::time::Instant;

pub async fn probe(url: &str) -> Result<(u16, u64)> {
    let started = Instant::now();
    let mut url = Url::parse(url)?;
    for hop in 0..=3 {
        let response = crate::outbound::client(&url, None, 15)
            .await?
            .get(url.clone())
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("Connection, timeout, or certificate validation failed")
            })?;
        if response.status().is_redirection() {
            if let Some(location) = response.headers().get("location") {
                ensure!(hop < 3, "Too many redirects");
                url = url.join(location.to_str()?)?;
                continue;
            }
        }
        return Ok((
            response.status().as_u16(),
            started.elapsed().as_millis() as u64,
        ));
    }
    unreachable!()
}

pub async fn fetch(url: &str) -> Result<(u16, u64, Url, Vec<u8>)> {
    let started = Instant::now();
    let mut url = Url::parse(url)?;
    for hop in 0..=3 {
        let response = crate::outbound::client(&url, None, 15)
            .await?
            .get(url.clone())
            .send()
            .await
            .map_err(|_| {
                anyhow::anyhow!("Connection, timeout, or certificate validation failed")
            })?;
        let status = response.status();
        if status.is_redirection() {
            if let Some(location) = response.headers().get("location") {
                ensure!(hop < 3, "Too many redirects");
                url = url.join(location.to_str().context("Invalid redirect")?)?;
                continue;
            }
        }
        let body = crate::outbound::bounded_body(response, 1024 * 1024).await?;
        return Ok((
            status.as_u16(),
            started.elapsed().as_millis() as u64,
            url,
            body,
        ));
    }
    unreachable!()
}
pub fn analyze(html: &str, url: &str) -> Value {
    let document = Html::parse_document(html);
    let first_text = |selector: &str| {
        document
            .select(&Selector::parse(selector).unwrap())
            .next()
            .map(|n| {
                n.text()
                    .collect::<String>()
                    .trim()
                    .chars()
                    .take(300)
                    .collect::<String>()
            })
            .unwrap_or_default()
    };
    let attr = |selector: &str, attribute: &str| {
        document
            .select(&Selector::parse(selector).unwrap())
            .next()
            .and_then(|n| n.value().attr(attribute))
            .unwrap_or("")
            .chars()
            .take(500)
            .collect::<String>()
    };
    let count = |selector: &str| document.select(&Selector::parse(selector).unwrap()).count();
    let title = first_text("title");
    let description = attr("meta[name='description']", "content");
    let canonical = attr("link[rel='canonical']", "href");
    let robots = attr("meta[name='robots']", "content");
    let checks = vec![
        json!({"name":"Page title","ok":!title.is_empty(),"detail":title}),
        json!({"name":"Meta description","ok":!description.is_empty(),"detail":description}),
        json!({"name":"Canonical URL","ok":!canonical.is_empty(),"detail":canonical}),
        json!({"name":"One primary heading","ok":count("h1")==1,"detail":format!("{} H1 elements",count("h1"))}),
        json!({"name":"Image alternative text","ok":count("img:not([alt])")==0,"detail":format!("{} images missing alt attributes",count("img:not([alt])"))}),
        json!({"name":"Document language","ok":!attr("html","lang").is_empty(),"detail":attr("html","lang")}),
        json!({"name":"Responsive viewport","ok":!attr("meta[name='viewport']","content").is_empty(),"detail":attr("meta[name='viewport']","content")}),
        json!({"name":"Open Graph title","ok":!attr("meta[property='og:title']","content").is_empty(),"detail":attr("meta[property='og:title']","content")}),
        json!({"name":"Indexing directive","ok":!robots.to_lowercase().contains("noindex"),"detail":if robots.is_empty(){"No meta robots directive".to_string()}else{robots}}),
    ];
    json!({"url":url,"checks":checks,"note":"Technical observations only; these checks do not predict search rankings."})
}
pub async fn seo(url: &str) -> Result<Value> {
    let (status, latency, final_url, body) = fetch(url).await?;
    let mut result = analyze(&String::from_utf8_lossy(&body), final_url.as_str());
    result["http_status"] = json!(status);
    result["response_ms"] = json!(latency);
    let mut discovery = Vec::new();
    for path in ["/robots.txt", "/sitemap.xml"] {
        let target = final_url.join(path)?;
        let info = match fetch(target.as_str()).await {
            Ok((code, _, _, _)) => json!({"path":path,"status":code,"available":code==200}),
            Err(_) => json!({"path":path,"available":false}),
        };
        discovery.push(info);
    }
    result["discovery"] = json!(discovery);
    Ok(result)
}
#[cfg(test)]
mod tests {
    #[test]
    fn observations_come_from_actual_markup() {
        let result = super::analyze(
            "<html lang='en'><title>Example</title><h1>Heading</h1><img src='a.png'></html>",
            "https://example.com",
        );
        assert_eq!(result["checks"][0]["detail"], "Example");
        assert_eq!(result["checks"][4]["ok"], false);
        assert_eq!(result["checks"][1]["ok"], false);
    }
}
