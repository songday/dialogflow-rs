use std::collections::HashMap;
use std::time::Duration;
use std::vec::Vec;

use reqwest::Client;
use reqwest::RequestBuilder;
use reqwest::header::{HeaderMap, HeaderName, HeaderValue};

use super::dto::{
    HttpReqInfo, HttpReqParam, Method, PostContentType, Protocol, ResponseData, ValueSource,
};
use crate::result::Result;
use crate::variable::dto::VariableValue;

pub(crate) fn get_client(
    connect_timeout_millis: u64,
    read_timeout_millis: u64,
    proxy_url: &str,
) -> Result<Client> {
    let mut client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(connect_timeout_millis))
        .read_timeout(Duration::from_millis(read_timeout_millis))
        // Since can not reuse Client currently, so set pool size to 0
        .pool_max_idle_per_host(0)
        .pool_idle_timeout(Duration::from_secs(1));
    if proxy_url.is_empty() {
        client = client.no_proxy();
    } else {
        let proxy = reqwest::Proxy::http(proxy_url)?;
        client = client.proxy(proxy);
    }
    let client = client.build()?;
    Ok(client)
}

pub(crate) async fn status_code(
    info: HttpReqInfo,
    timeout_milliseconds: u64,
    vars: HashMap<String, VariableValue>,
) -> Result<u16> {
    let req = build_req(&info, timeout_milliseconds, &vars)?;
    let res = req.send().await?;
    Ok(res.status().as_u16())
}

pub(crate) async fn req(
    info: HttpReqInfo,
    timeout_milliseconds: u64,
    vars: &HashMap<String, VariableValue>,
) -> reqwest::Result<ResponseData> {
    let req = build_req(&info, timeout_milliseconds, vars)?;
    let res = req.send().await?;
    // println!("http status code {}", res.status().as_str());
    if res.status() != reqwest::StatusCode::OK {
        return Ok(ResponseData::None);
    }
    let content_type = res
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .map_or("", |h| h.to_str().unwrap());
    let data = if content_type.contains("text/")
        || content_type.contains("/json")
        || content_type.contains("/xml")
    {
        let s = res.text().await?;
        // println!("{}", s);
        ResponseData::Str(s)
    } else {
        ResponseData::Bin(res.bytes().await?.to_vec())
    };
    Ok(data)
}

/// The request body as the wire should carry it: the rich text the editor
/// stores (variable chips) reduced to plain text, with the values of every
/// referenced variable filled in — the same `vars` the headers and query
/// parameters resolve against. An unknown variable is left as the `` `name` ``
/// it was written as.
fn resolve_body(body: &str, vars: &HashMap<String, VariableValue>) -> String {
    let plain = crate::flow::rt::var_replace::rich_text_body_to_plain(body);
    match crate::flow::rt::var_replace::replace_vars_with(&plain, |name| {
        Ok(vars.get(name).map(|v| v.val_to_string()))
    }) {
        Ok(s) => s,
        // The resolver above never fails; keep the plain text either way.
        Err(_) => plain,
    }
}

/// Turn a parameter table into the `(name, value)` pairs reqwest encodes, for
/// a query string or a form body alike. A variable-sourced parameter takes the
/// current value of that variable, or an empty string when it is not set.
fn pairs<'a>(
    params: &'a [HttpReqParam],
    vars: &HashMap<String, VariableValue>,
) -> Vec<(&'a str, String)> {
    params
        .iter()
        .map(|p| {
            let value = match p.value_source {
                ValueSource::Val => p.value.clone(),
                ValueSource::Var => vars
                    .get(&p.value)
                    .map_or(String::new(), |v| v.val_to_string()),
            };
            (p.name.as_str(), value)
        })
        .collect()
}

fn build_req(
    info: &HttpReqInfo,
    timeout_milliseconds: u64,
    vars: &HashMap<String, VariableValue>,
) -> reqwest::Result<RequestBuilder> {
    let client = reqwest::Client::builder()
        .connect_timeout(Duration::from_millis(1000))
        .read_timeout(Duration::from_millis(timeout_milliseconds))
        .build()?;
    let mut url = String::with_capacity(512);
    match info.protocol {
        Protocol::HTTP => url.push_str("http"),
        Protocol::HTTPS => url.push_str("https"),
    }
    url.push_str("://");
    url.push_str(&info.address);
    let mut req = match info.method {
        Method::GET => client.get(&url),
        Method::POST => {
            let r = client.post(&url);
            if info.post_content_type == PostContentType::UrlEncoded {
                // The body is the parameter table. `request_body` belongs to
                // RAW and is not editable in this mode.
                if info.form_data.is_empty() {
                    r
                } else {
                    r.form(&pairs(&info.form_data, vars))
                }
            } else {
                let body = resolve_body(&info.request_body, vars);
                if body.is_empty() { r } else { r.body(body) }
            }
        }
    };
    if !info.headers.is_empty() {
        let mut headers: HeaderMap<HeaderValue> = HeaderMap::with_capacity(info.headers.len());
        for p in info.headers.iter() {
            match p.value_source {
                ValueSource::Val => headers.insert(
                    HeaderName::from_bytes(p.name.as_bytes()).unwrap(),
                    p.value.parse().unwrap(),
                ),
                ValueSource::Var => headers.insert(
                    HeaderName::from_bytes(p.name.as_bytes()).unwrap(),
                    vars.get(&p.value)
                        .map_or(String::new(), |v| v.val_to_string())
                        .parse()
                        .unwrap(),
                ),
            };
        }
        req = req.headers(headers);
    }
    if !info.query_params.is_empty() {
        req = req.query(&pairs(&info.query_params, vars));
    }
    if info.post_content_type == PostContentType::Raw {
        // The body is whatever the user typed. Only the default content type
        // assumes JSON; an empty `content_type` is a record saved before the
        // field existed, and those were sent as application/json.
        let content_type = info.content_type.trim();
        req = req.header(
            "Content-Type",
            if content_type.is_empty() {
                "application/json"
            } else {
                content_type
            },
        );
    }
    if !info.user_agent.is_empty() {
        req = req.header("User-Agent", &info.user_agent);
    }
    // Ok(req.timeout(Duration::from_millis(info.timeout_milliseconds)))
    Ok(req)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::variable::dto::VariableType;

    fn vars() -> HashMap<String, VariableValue> {
        let mut vars = HashMap::new();
        vars.insert(
            String::from("name"),
            VariableValue::new("Ada", &VariableType::Str),
        );
        vars.insert(
            String::from("count"),
            VariableValue::new("7", &VariableType::Num),
        );
        vars
    }

    #[test]
    fn pairs_resolve_variables() {
        let p = |name: &str, value: &str, source: ValueSource| HttpReqParam {
            name: String::from(name),
            value: String::from(value),
            value_source: source,
        };
        let params = vec![
            p("a", "x", ValueSource::Val),
            p("b", "name", ValueSource::Var),
            // Known variable of another kind, and one that does not exist.
            p("c", "count", ValueSource::Var),
            p("d", "nope", ValueSource::Var),
        ];
        assert_eq!(
            pairs(&params, &vars()),
            vec![
                ("a", String::from("x")),
                ("b", String::from("Ada")),
                ("c", String::from("7")),
                ("d", String::new()),
            ]
        );
    }

    #[test]
    fn body_from_editor_is_plain_and_substituted() {
        let body = "<p>{\"name\": \"<var class=\"var-chip\" data-var-name=\"name\" data-var-type=\"String\">name</var>\", \"count\": <var data-var-name=\"count\">count</var>}</p>";
        assert_eq!(
            resolve_body(body, &vars()),
            "{\"name\": \"Ada\", \"count\": 7}\n"
        );
    }

    #[test]
    fn body_keeps_unknown_variable() {
        let body = "<p><var data-var-name=\"nope\">nope</var></p>";
        assert_eq!(resolve_body(body, &vars()), "`nope`\n");
    }

    #[test]
    fn legacy_plain_body_is_substituted() {
        assert_eq!(
            resolve_body("{\"name\": \"`name`\"}", &vars()),
            "{\"name\": \"Ada\"}"
        );
        // …and text without any variable reference is sent as written.
        assert_eq!(
            resolve_body("{\"a\": \"a<b\"}", &vars()),
            "{\"a\": \"a<b\"}"
        );
    }
}
